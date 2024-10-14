use crate::models::rrq::produce::ProducePayload;
use crate::models::rrq::queue::QueuePayload;
use crate::models::server::extract::ExtractionPayload;
use crate::models::server::segment::{PdlaSegment, Segment};
use crate::models::server::task::Status;
use crate::task::pdf::convert_to_pdf;
use crate::task::pdf::split_pdf;
use crate::task::pdla::pdla_extraction;
use crate::utils::configs::extraction_config::Config;
use crate::utils::db::deadpool_postgres::{create_pool, Client, Pool};
use crate::utils::rrq::service::produce;
use crate::utils::storage::config_s3::create_client;
use crate::utils::storage::services::{download_to_tempfile, upload_to_s3};
use anyhow::anyhow;
use chrono::{DateTime, Utc};
use lopdf::Document;
use std::error::Error;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use tempdir::TempDir;
use tempfile::NamedTempFile;
use uuid::Uuid;

pub async fn produce_ocr_payload(
    extraction_payload: ExtractionPayload,
) -> Result<(), Box<dyn Error>> {
    let config = Config::from_env()?;

    let queue_name = config
        .extraction_queue_ocr
        .ok_or_else(|| "OCR queue name not configured".to_string())?;

    let produce_payload = ProducePayload {
        queue_name: queue_name.clone(),
        publish_channel: None,
        payload: serde_json::to_value(extraction_payload)?,
        max_attempts: None,
        item_id: Uuid::new_v4().to_string(),
    };

    produce(vec![produce_payload]).await?;
    println!("Produced OCR payload");
    Ok(())
}

fn is_valid_file_type(original_file_name: &str) -> Result<(bool, String), Box<dyn Error>> {
    let extension = Path::new(original_file_name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("");

    let is_valid = match extension.to_lowercase().as_str() {
        "pdf" | "docx" | "doc" | "pptx" | "ppt" | "xlsx" | "xls" => true,
        _ => false,
    };

    Ok((is_valid, format!("application/{}", extension)))
}

pub async fn log_task(
    task_id: String,
    status: Status,
    message: Option<String>,
    finished_at: Option<DateTime<Utc>>,
    pool: &Pool,
) -> Result<(), Box<dyn std::error::Error>> {
    let client: Client = pool.get().await?;

    let task_query = format!(
        "UPDATE tasks SET status = '{:?}', message = '{}', finished_at = '{:?}' WHERE task_id = '{}'",
        status,
        message.unwrap_or_default(),
        finished_at.unwrap_or_default(),
        task_id
    );

    client.execute(&task_query, &[]).await?;

    Ok(())
}

// Start of Selection
pub async fn preprocess(
    s3_client: &aws_sdk_s3::Client,
    reqwest_client: &reqwest::Client,
    extraction_item: &ExtractionPayload,
    task_id: &str,
    user_id: &str,
    client: &Client,
) -> Result<(PathBuf, String, i32, String), Box<dyn Error>> {
    let temp_file = download_to_tempfile(
        s3_client,
        reqwest_client,
        &extraction_item.input_location,
        None,
    )
    .await?;

    let (is_valid, detected_mime_type) = is_valid_file_type(&extraction_item.input_location)?;
    if !is_valid {
        return Err(format!("Not a valid file type: {}", detected_mime_type).into());
    }
    let extension = detected_mime_type.split('/').nth(1).unwrap_or("pdf");
    let original_path = PathBuf::from(temp_file.path());
    let mut final_output_path: PathBuf = original_path.clone();
    let final_output_file = NamedTempFile::new()?;

    let output_file = NamedTempFile::new()?;
    let output_path = output_file.path().to_path_buf();

    if extension != "pdf" {
        let new_path = original_path.with_extension(extension).clone();

        std::fs::rename(&original_path, &new_path)?;

        let input_path = new_path;

        let result = convert_to_pdf(&input_path, &output_path).await;
        final_output_path = final_output_file.path().to_path_buf();

        match result {
            Ok(_) => {
                std::fs::copy(&output_path, &final_output_path).unwrap();
            }
            Err(e) => {
                println!("PDF conversion failed: {:?}", e);
                return Err(e.into());
            }
        }
    }
    let config = Config::from_env()?;

    let s3_pdf_location = format!(
        "s3://{}/{}/{}/{}",
        config.s3_bucket,
        user_id,
        task_id,
        if extraction_item.input_location.ends_with(".pdf") {
            extraction_item.input_location.to_string()
        } else {
            format!("{}.pdf", extraction_item.input_location)
        }
    );
    let page_count = match Document::load(&final_output_path) {
        Ok(doc) => doc.get_pages().len() as i32,
        Err(e) => {
            return Err(format!("Failed to get page count: {}", e).into());
        }
    };

    upload_to_s3(s3_client, &s3_pdf_location, &final_output_path)
        .await
        .map_err(|e| {
            anyhow::anyhow!("Failed to upload PDF to S3 at {}: {:?}", s3_pdf_location, e)
        })?;

    client
            .execute(
                "UPDATE tasks SET pdf_location = $1, page_count = $2, input_file_type = $3 WHERE task_id = $4",
                &[&s3_pdf_location, &page_count, &extension, &task_id],
            )
            .await?;

    Ok((
        final_output_path,
        s3_pdf_location,
        page_count,
        extension.to_string(),
    ))
}

pub async fn process(payload: QueuePayload) -> Result<(), Box<dyn std::error::Error>> {
    println!("Processing task");
    let s3_client: aws_sdk_s3::Client = create_client().await?;
    let reqwest_client = reqwest::Client::new();
    let extraction_item: ExtractionPayload = serde_json::from_value(payload.payload)?;
    let task_id = extraction_item.task_id.clone();
    let user_id = extraction_item.user_id.clone();
    let pg_pool = create_pool();
    let client: Client = pg_pool.get().await?;
    let file_name_query = "SELECT file_name FROM tasks WHERE task_id = $1 AND user_id = $2";
    let file_name_row = client
        .query_one(file_name_query, &[&task_id, &user_id])
        .await?;
    let file_name: String = file_name_row.get(0);

    log_task(
        task_id.clone(),
        Status::Processing,
        Some(format!(
            "Task processing | Tries ({}/{})",
            payload.attempt, payload.max_attempts
        )),
        None,
        &pg_pool,
    )
    .await?;

    let result: Result<(), Box<dyn std::error::Error>> = (async {
        let (final_output_path, s3_pdf_location, page_count, extension) = preprocess(
            &s3_client,
            &reqwest_client,
            &extraction_item,
            &task_id,
            &user_id,
            &client,
        )
        .await?;

        let mut split_temp_files: Vec<PathBuf> = Vec::new();
        let split_temp_dir = TempDir::new("split_pdf")?;

        if let Some(batch_size) = extraction_item.batch_size {
            split_temp_files = split_pdf(
                &final_output_path,
                batch_size as usize,
                split_temp_dir.path(),
            )
            .await?;
        } else {
            split_temp_files.push(final_output_path.clone());
        }

        let mut combined_output: Vec<Segment> = Vec::new();
        let mut page_offset: u32 = 0;
        let mut batch_number: i32 = 0;

        for temp_file in &split_temp_files {
            batch_number += 1;
            let segmentation_message = if split_temp_files.len() > 1 {
                format!(
                    "Segmenting | Batch {} of {}",
                    batch_number,
                    split_temp_files.len()
                )
            } else {
                "Segmenting".to_string()
            };

            log_task(
                task_id.clone(),
                Status::Processing,
                Some(segmentation_message),
                None,
                &pg_pool,
            )
            .await?;

            let temp_file_path = temp_file.to_path_buf();

            let pdla_response =
                pdla_extraction(&temp_file_path, extraction_item.model.clone()).await?;
            let pdla_segments: Vec<PdlaSegment> = serde_json::from_str(&pdla_response)?;
            let mut segments: Vec<Segment> = pdla_segments
                .iter()
                .map(|pdla_segment| pdla_segment.to_segment())
                .collect();

            for item in &mut segments {
                item.page_number += page_offset;
            }
            combined_output.extend(segments);
            page_offset += extraction_item.batch_size.unwrap_or(1) as u32;
        }

        let mut output_temp_file = NamedTempFile::new()?;
        output_temp_file.write_all(serde_json::to_string(&combined_output)?.as_bytes())?;
        println!(
            "Output file written: {:?}, Size: {} bytes",
            output_temp_file,
            std::fs::metadata(output_temp_file.path())?.len()
        );
        upload_to_s3(
            &s3_client,
            &extraction_item.output_location,
            &output_temp_file.path(),
        )
        .await?;

        if output_temp_file.path().exists() {
            if let Err(e) = std::fs::remove_file(output_temp_file.path()) {
                eprintln!("Error deleting temporary file: {:?}", e);
            }
        }

        Ok(())
    })
    .await;

    match result {
        Ok(_) => {
            println!("Task succeeded");

            produce_ocr_payload(extraction_item).await?;

            Ok(())
        }
        Err(e) => {
            eprintln!("Error processing task: {:?}", e);
            let error_message = if e
                .to_string()
                .to_lowercase()
                .contains("usage limit exceeded")
            {
                "Task failed: Usage limit exceeded".to_string()
            } else {
                "Task failed".to_string()
            };

            if payload.attempt >= payload.max_attempts {
                eprintln!("Task failed after {} attempts", payload.max_attempts);
                log_task(
                    task_id.clone(),
                    Status::Failed,
                    Some(error_message),
                    Some(Utc::now()),
                    &pg_pool,
                )
                .await?;
            }
            Err(e)
        }
    }
}

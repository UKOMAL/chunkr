import { Flex, Table } from "@radix-ui/themes";
import "./ModelTable.css";

export default function ModelTable() {
  return (
    <Flex direction="column" align="center" justify="center" width="100%">
      <Table.Root variant="surface" mt="24px" style={{ width: "100%" }}>
        <Table.Header>
          <Table.Row>
            <Table.ColumnHeaderCell>FEATURE</Table.ColumnHeaderCell>
            <Table.ColumnHeaderCell>FAST MODEL</Table.ColumnHeaderCell>
            <Table.ColumnHeaderCell>HIGH QUALITY MODEL</Table.ColumnHeaderCell>
          </Table.Row>
        </Table.Header>
        <Table.Body>
          <Table.Row>
            <Table.RowHeaderCell className="sub-header">
              Table extraction
            </Table.RowHeaderCell>
            <Table.Cell>✓</Table.Cell>
            <Table.Cell>✓</Table.Cell>
          </Table.Row>
          <Table.Row>
            <Table.RowHeaderCell className="sub-header">
              Image processing
            </Table.RowHeaderCell>
            <Table.Cell>Basic</Table.Cell>
            <Table.Cell>Advanced</Table.Cell>
          </Table.Row>
          <Table.Row>
            <Table.RowHeaderCell className="sub-header">
              Formula OCR
            </Table.RowHeaderCell>
            <Table.Cell>-</Table.Cell>
            <Table.Cell>✓</Table.Cell>
          </Table.Row>
          <Table.Row>
            <Table.RowHeaderCell className="sub-header">
              Processing speed
            </Table.RowHeaderCell>
            <Table.Cell>Very fast</Table.Cell>
            <Table.Cell>Moderate</Table.Cell>
          </Table.Row>
          <Table.Row>
            <Table.RowHeaderCell className="sub-header">
              Accuracy
            </Table.RowHeaderCell>
            <Table.Cell>Good</Table.Cell>
            <Table.Cell>Excellent</Table.Cell>
          </Table.Row>
        </Table.Body>
      </Table.Root>
    </Flex>
  );
}
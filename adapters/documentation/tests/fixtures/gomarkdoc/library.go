// Package reference preserves a small public library contract.
//
// Read the [guide] before constructing a Record.
//
// [guide]: https://example.invalid/guide
package reference

// DefaultLabel is the original label for a new Record.
const DefaultLabel = "hello"

// Record retains an original label without conversion.
type Record struct {
	// Value is the original payload.
	Value string
}

// New constructs a Record with the supplied label.
func New(value string) Record { return Record{Value: value} }

// Label returns the original payload without conversion.
func (record Record) Label() string { return record.Value }

// hidden is deliberately excluded from the public reference.
func hidden() {}

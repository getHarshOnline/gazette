package reference_test

import (
	"fmt"

	"example.invalid/reference"
)

func ExampleNew() {
	record := reference.New(reference.DefaultLabel)
	fmt.Println(record.Label())
	// Output: hello
}

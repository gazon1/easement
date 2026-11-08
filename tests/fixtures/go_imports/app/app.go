package app

import "fmt"

import (
	// the module's own package, by an alias
	lb "example.com/forms/lib"
	. "example.com/forms/dot" /* dot */
	_ "strings"

	"example.com/ext"
)

func A() { fmt.Println(lb.L(), D(), ext.E()) }

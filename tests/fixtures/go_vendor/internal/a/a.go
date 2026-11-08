package a

import "example.com/dep"

// Greet greets.
func Greet() string { return dep.Hello() }

package app

// Two imports on one line, each resolved on its own path.
import ( "os"; lb2 "example.com/forms/lib" )

var _, _ = os.Args, lb2.L

package lib

import "example.com/app/internal/domain/tags"

func Clean() {
	tags.DeleteIn(2)
}

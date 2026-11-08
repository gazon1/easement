package web

import "example.com/app/internal/domain/tags"

func Delete(id int64) {
	tags.DeleteIn(id)
}

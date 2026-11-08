package contacts

import "example.com/app/internal/domain/tags"

func Remove(id int64) {
	tags.DeleteIn(id)
}

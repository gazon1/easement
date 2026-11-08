package tagdeletion

import "example.com/app/internal/domain/tags"

func Run(id int64) {
	tags.DeleteIn(id)
}

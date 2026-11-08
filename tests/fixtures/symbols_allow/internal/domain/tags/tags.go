package tags

// DeleteIn deletes the tags of a contact.
func DeleteIn(contact int64) {}

// Purge deletes every tag; a call inside the defining file is not a use.
func Purge() {
	DeleteIn(0)
}

# Fixture of the update-key tests: the spare key is RFC 8032 section 7.1 TEST 2, whose private half
# is published. A spare is a key that can sign too, so it is refused like the active key.
set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS OFF)
set(KATANA_UPDATE_KEY_ACTIVE "97fcb0a317920f5d988416cb98ba6937886975cc6b4a35d65a4b244c82d76428")
set(KATANA_UPDATE_KEY_SPARE "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c")

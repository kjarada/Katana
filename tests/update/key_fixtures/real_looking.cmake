# Fixture of the update-key tests (tests/update/CMakeLists.txt): what the file looks like
# after the owner's key steps - two different public keys and the flag off. These two
# were made for this fixture with `openssl genpkey -algorithm ed25519`, and their private
# halves were discarded: they are public data, and they sign nothing.
set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS OFF)
set(KATANA_UPDATE_KEY_ACTIVE "97fcb0a317920f5d988416cb98ba6937886975cc6b4a35d65a4b244c82d76428")
set(KATANA_UPDATE_KEY_SPARE "393a4825f86253bae6d9e88301fea90cf2b350ba910225a6b3a92c445b37e477")

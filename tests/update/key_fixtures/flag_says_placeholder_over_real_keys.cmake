# Fixture of the update-key tests: the flag still says placeholders over real keys, as a
# hand edit that forgot it would leave it. It would refuse a release for no reason anyone
# could find, so the file is refused as inconsistent.
set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS ON)
set(KATANA_UPDATE_KEY_ACTIVE "97fcb0a317920f5d988416cb98ba6937886975cc6b4a35d65a4b244c82d76428")
set(KATANA_UPDATE_KEY_SPARE "393a4825f86253bae6d9e88301fea90cf2b350ba910225a6b3a92c445b37e477")

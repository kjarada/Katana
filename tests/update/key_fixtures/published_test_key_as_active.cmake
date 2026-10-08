# Fixture of the update-key tests: the active key is RFC 8032 section 7.1 TEST 1, whose private half
# is published. Whoever pinned it - the tool with --allow-test-keys, or a hand edit - the configure
# must refuse it, release or not.
set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS OFF)
set(KATANA_UPDATE_KEY_ACTIVE "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
set(KATANA_UPDATE_KEY_SPARE "393a4825f86253bae6d9e88301fea90cf2b350ba910225a6b3a92c445b37e477")

# Fixture of the update-key tests: the flag says the keys are real over bytes that spell
# PLACEHOLDER. Were that let through, KATANA_REQUIRE_UPDATE_KEYS would pass a release
# whose keys nobody holds.
set(KATANA_UPDATE_KEYS_ARE_PLACEHOLDERS OFF)
set(KATANA_UPDATE_KEY_ACTIVE "504c414345484f4c4445522d4143544956452d4e4f542d412d4b45592d30302d")
set(KATANA_UPDATE_KEY_SPARE "393a4825f86253bae6d9e88301fea90cf2b350ba910225a6b3a92c445b37e477")

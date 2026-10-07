// A static library that links the legacy customisation readers PRIVATELY, for the
// link-closure check to be proved on (tests/CMakeLists.txt). It is never built:
// the target is EXCLUDE_FROM_ALL, and the check reads only what CMake knows of it.
int katana_link_closure_fixture = 0;

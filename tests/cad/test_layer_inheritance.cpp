// Visibility and lock inherit DOWN the layer tree (PLAN.MD section 10, 5.1): a
// layer switched off or locked by an ancestor must be undrawn, unpickable and
// uneditable exactly as though it had been switched off or locked itself.
// Populated by the fix for the audit finding of 2026-09-23 that the
// inheritance was enforced only by the layer panel.

#include <gtest/gtest.h>

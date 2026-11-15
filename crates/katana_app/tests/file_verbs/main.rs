//! The twins of tests/app/test_survey_verbs.cpp (SURVEY IMPORT's reduction
//! settings, the held control and the adjustment's records; what the import
//! does with the survey codes) and tests/app/test_ifc_verbs.cpp (the road
//! model as the IFC export takes it): one module per C++ file, the gtest
//! suites as modules inside.
//!
//! Every case drives a verb function on a Document of its own and writes only
//! in a scratch folder of its own, so the cases run side by side on libtest's
//! threads with no lock: nothing here sets an environment variable, changes
//! the working directory or reads the real streams.

mod ifc;
mod survey;

katana_add_test_suite(math
    LIBS katana_math
    SOURCES
        test_numerics.cpp
        test_vectors.cpp
        test_matrices.cpp
        test_rotation.cpp
        test_primitives.cpp test_summation.cpp
        test_unit_ratio.cpp
)

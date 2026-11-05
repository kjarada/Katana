mod simd_facade {
    use katana_core::simd;

    fn check_integer_vectors<T: simd::Element + std::fmt::Debug>(left: &[T], right: &[T]) {
        let vector_left = simd::load(left);
        let vector_right = simd::load(right);
        let mut minimum = vec![T::default(); simd::lanes::<T>()];
        let mut maximum = minimum.clone();
        simd::store(&simd::min_of(&vector_left, &vector_right), &mut minimum);
        simd::store(&simd::max_of(&vector_left, &vector_right), &mut maximum);
        for index in 0..simd::lanes::<T>() {
            assert_eq!(
                minimum[index],
                if right[index] < left[index] {
                    right[index]
                } else {
                    left[index]
                }
            );
            assert_eq!(
                maximum[index],
                if left[index] < right[index] {
                    right[index]
                } else {
                    left[index]
                }
            );
        }
    }

    #[test]
    fn integer_masks_preserve_signed_and_unsigned_lane_order() {
        check_integer_vectors(
            &[-128i8, 127, -1, 0, 1, -2, 2, -3, 3, -4, 4, -5, 5, -6, 6, -7],
            &[127i8, -128, 0, -1, -2, 1, -3, 2, -4, 3, -5, 4, -6, 5, -7, 6],
        );
        check_integer_vectors(
            &[
                0u8, 255, 127, 128, 1, 254, 2, 253, 3, 252, 4, 251, 5, 250, 6, 249,
            ],
            &[
                255u8, 0, 128, 127, 254, 1, 253, 2, 252, 3, 251, 4, 250, 5, 249, 6,
            ],
        );
        check_integer_vectors(
            &[i16::MIN, i16::MAX, -1, 0, 1, -2, 2, -3],
            &[i16::MAX, i16::MIN, 0, -1, -2, 1, -3, 2],
        );
        check_integer_vectors(
            &[0u16, u16::MAX, 32767, 32768, 1, 65534, 2, 65533],
            &[u16::MAX, 0, 32768, 32767, 65534, 1, 65533, 2],
        );
        check_integer_vectors(&[i32::MIN, i32::MAX, -1, 0], &[i32::MAX, i32::MIN, 0, -1]);
        check_integer_vectors(
            &[0u32, u32::MAX, 0x7fff_ffff, 0x8000_0000],
            &[u32::MAX, 0, 0x8000_0000, 0x7fff_ffff],
        );
        check_integer_vectors(&[i64::MIN, i64::MAX], &[i64::MAX, i64::MIN]);
        check_integer_vectors(
            &[0x0000_0000_ffff_ffffi64, 0x0000_0001_8000_0000],
            &[0x0000_0001_0000_0000i64, 0x0000_0001_7fff_ffff],
        );
        check_integer_vectors(&[0u64, u64::MAX], &[u64::MAX, 0]);
        check_integer_vectors(
            &[0x7fff_ffff_ffff_ffffu64, 0x8000_0000_0000_0000],
            &[0x8000_0000_0000_0000u64, 0x7fff_ffff_ffff_ffff],
        );
        check_integer_vectors(
            &vec![isize::MIN; simd::lanes::<isize>()],
            &vec![isize::MAX; simd::lanes::<isize>()],
        );
        check_integer_vectors(
            &vec![0usize; simd::lanes::<usize>()],
            &vec![usize::MAX; simd::lanes::<usize>()],
        );
    }

    #[test]
    fn the_backend_is_std_simd_exactly_when_the_library_provides_it() {
        assert_eq!(simd::BACKEND, "portable");
    }

    #[test]
    fn baseline_code_gets_whole_sse2_registers_of_every_type() {
        assert_eq!(simd::lanes::<f64>() * std::mem::size_of::<f64>(), 16);
        assert_eq!(simd::lanes::<f32>() * std::mem::size_of::<f32>(), 16);
    }

    #[test]
    fn load_and_store_copy_exactly_one_vector_and_need_no_alignment() {
        let count = simd::lanes::<f64>();
        let input: Vec<_> = (0..count + 2).map(|index| 1.5 * index as f64).collect();
        let mut output = vec![-7.0; count + 2];
        simd::store(&simd::load(&input[1..]), &mut output[1..]);
        assert_eq!(output[0], -7.0);
        assert_eq!(output[output.len() - 1], -7.0);
        for index in 1..=count {
            assert_eq!(output[index], input[index]);
        }
    }

    #[test]
    fn min_of_and_max_of_are_std_min_and_std_max_lane_by_lane_even_for_na_n_and_zeros() {
        let nan = f64::NAN;
        let left = [0.0, -0.0, 1.0, nan, 2.0, -4.0];
        let right = [-0.0, 0.0, nan, 3.0, 1.0, 5.0];
        let expected_min = [0.0, -0.0, 1.0, nan, 1.0, -4.0];
        let expected_max = [0.0, -0.0, 1.0, nan, 2.0, 5.0];
        let count = simd::lanes::<f64>();
        let mut padded_left = left.to_vec();
        let mut padded_right = right.to_vec();
        while !padded_left.len().is_multiple_of(count) {
            padded_left.push(0.0);
            padded_right.push(0.0);
        }
        let mut got_min = vec![0.0f64; padded_left.len()];
        let mut got_max = vec![0.0f64; padded_left.len()];
        for index in (0..padded_left.len()).step_by(count) {
            let vector_left = simd::load(&padded_left[index..]);
            let vector_right = simd::load(&padded_right[index..]);
            simd::store(
                &simd::min_of(&vector_left, &vector_right),
                &mut got_min[index..],
            );
            simd::store(
                &simd::max_of(&vector_left, &vector_right),
                &mut got_max[index..],
            );
        }
        for index in 0..left.len() {
            assert_eq!(
                got_min[index].to_bits(),
                expected_min[index].to_bits(),
                "lane {index}"
            );
            assert_eq!(
                got_max[index].to_bits(),
                expected_max[index].to_bits(),
                "lane {index}"
            );
            let cpp_min = if right[index] < left[index] {
                right[index]
            } else {
                left[index]
            };
            let cpp_max = if left[index] < right[index] {
                right[index]
            } else {
                left[index]
            };
            assert_eq!(got_min[index].to_bits(), cpp_min.to_bits(), "lane {index}");
            assert_eq!(got_max[index].to_bits(), cpp_max.to_bits(), "lane {index}");
        }
    }

    #[test]
    fn select_takes_the_first_where_the_mask_is_set_and_the_second_elsewhere() {
        let count = simd::lanes::<f32>();
        let left: Vec<_> = (0..count).map(|index| index as f32).collect();
        let right: Vec<_> = (0..count).map(|index| (count - 1 - index) as f32).collect();
        let vector_left = simd::load(&left);
        let vector_right = simd::load(&right);
        let mut output = vec![0.0; count];
        simd::store(
            &simd::select(
                &vector_left.less_than(&vector_right),
                &vector_left,
                &vector_right,
            ),
            &mut output,
        );
        for index in 0..count {
            assert_eq!(
                output[index],
                if right[index] < left[index] {
                    right[index]
                } else {
                    left[index]
                },
                "lane {index}"
            );
        }
    }
}

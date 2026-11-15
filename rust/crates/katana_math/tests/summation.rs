#[macro_use]
mod common;

mod compensated_sum {
    use katana_math::CompensatedSum;

    #[test]
    fn keeps_a_small_term_that_plain_addition_would_lose() {
        let mut plain = 1e16;
        plain += 1.0;
        plain -= 1e16;
        assert_eq!(plain, 0.0, "the premise: plain addition loses it");
        let mut sum = CompensatedSum::default();
        sum.add(1e16);
        sum.add(1.0);
        sum.add(-1e16);
        assert_eq!(sum.value(), 1.0);
    }

    #[test]
    fn small_terms_added_to_a_large_running_sum_are_not_rounded_away() {
        let mut plain = 1e15;
        for _ in 0..1000000 {
            plain += 0.1;
        }
        plain -= 1e15;
        assert_near!(plain, 125000.0, 1.0);
        let mut sum = CompensatedSum::default();
        sum.add(1e15);
        for _ in 0..1000000 {
            sum.add(0.1);
        }
        sum.add(-1e15);
        assert_near!(sum.value(), 100000.0, 1e-6);
    }

    #[test]
    fn an_empty_sum_is_zero_and_order_does_not_change_the_answer() {
        let empty = CompensatedSum::default();
        assert_eq!(empty.value(), 0.0);
        let terms = [1e10, 3.25, -1e10, 0.125, 7.0, -3.375, 1e-3];
        let mut forward = CompensatedSum::default();
        for term in terms {
            forward.add(term);
        }
        let mut backward = CompensatedSum::default();
        for &term in terms.iter().rev() {
            backward.add(term);
        }
        assert_near!(forward.value(), backward.value(), 1e-12);
        assert_near!(forward.value(), 7.001, 1e-12);
    }
}

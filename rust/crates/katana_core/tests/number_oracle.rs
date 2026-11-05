use katana_core::text::{format_exact_real, format_fixed, format_general};
use std::io::{BufWriter, Write};
use std::process::{Command, Stdio};

const DECIMALS: [i32; 6] = [-1, 0, 3, 12, 60, 61];

fn corpus() -> Vec<u64> {
    let mut cases = vec![
        0.0_f64.to_bits(),
        (-0.0_f64).to_bits(),
        0.1_f64.to_bits(),
        1.2345678901234567_f64.to_bits(),
        1e20_f64.to_bits(),
        1e-4_f64.to_bits(),
        1e-5_f64.to_bits(),
        f64::MAX.to_bits(),
        f64::MIN_POSITIVE.to_bits(),
        f64::INFINITY.to_bits(),
        f64::NEG_INFINITY.to_bits(),
        f64::NAN.to_bits(),
        (-f64::NAN).to_bits(),
    ];
    for exponent in 0..=0x7ff_u64 {
        for mantissa in [0, 1, 0x000f_ffff_ffff_ffff] {
            let bits = (exponent << 52) | mantissa;
            cases.extend([bits, bits | (1_u64 << 63)]);
        }
    }
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    for _ in 0..16_384 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        cases.push(state);
    }
    for value in -128..=128 {
        cases.push((f64::from(value) + 0.5).to_bits());
    }
    cases
}

#[test]
fn number_formats_equal_cpp_on_every_exponent_and_random_doubles() {
    let oracle = std::env::var_os("KATANA_NUMBER_FORMAT_ORACLE")
        .expect("run tools/build_visible.sh wsl so the C++ number-format oracle is available");
    let cases = corpus();
    let mut child = Command::new(oracle)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the C++ number-format oracle must be built before the cargo stage");
    let input = child.stdin.take().unwrap();
    let output = std::thread::scope(|scope| {
        let writer = scope.spawn(|| {
            let mut writer = BufWriter::new(input);
            for bits in &cases {
                writeln!(writer, "{bits:016x}").unwrap();
            }
            writer.flush().unwrap();
        });
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        output
    });
    assert!(
        output.status.success(),
        "oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), cases.len(), "oracle must answer every input");
    for (bits, line) in cases.iter().zip(lines) {
        let value = f64::from_bits(*bits);
        let expected: Vec<_> = line.split('\t').collect();
        assert_eq!(expected.len(), DECIMALS.len() + 2);
        assert_eq!(
            format_exact_real(value),
            expected[0],
            "exact bits={bits:016x}"
        );
        assert_eq!(
            format_general(value, 12),
            expected[1],
            "general bits={bits:016x}"
        );
        for (index, decimals) in DECIMALS.iter().enumerate() {
            assert_eq!(
                format_fixed(value, *decimals),
                expected[index + 2],
                "fixed bits={bits:016x} decimals={decimals}"
            );
        }
    }
}

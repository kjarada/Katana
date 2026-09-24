#pragma once

// The formulas of the reduction, one function each, so that every number the
// report prints comes from one place and the report can name the formula.
// Private to src/katana_survey: the public face of these is the report
// (reduction_report.hpp), which records what each one did to each value.

#include <cstddef>
#include <optional>

namespace katana::survey::detail {

// ---- Atmospheric correction ----------------------------------------------------------
//
// Manufacturer-neutral: the group refractivity of air for the EDM's carrier by
// the IUGG 1999 resolution (Ciddor & Hill 1999, the formula the IAG adopted for
// electronic distance measurement):
//
//   N_gr   = 287.6155 + 4.88660 / L^2 + 0.06800 / L^4        (L in micrometres;
//            standard air: 0 C, 1013.25 hPa, dry, 375 ppm CO2)
//   N_L    = (273.15 / 1013.25) N_gr p / T  -  11.27 e / T   (T kelvin, p and e hPa)
//   e      = RH / 100 * 6.1078 * 10^(7.5 t / (237.3 + t))   (Magnus-Tetens, over water)
//   ppm    = N_ref - N_L
//
// where N_ref is N_L at the instrument's reference atmosphere, the atmosphere
// in which its displayed distance needs no correction. Nothing in a field file
// states the carrier or the reference atmosphere, so both are constants here:
// a 658 nm carrier and 12 C, 1013.25 hPa, 60 % relative humidity, which gives
// N_ref = 286.338 - the reference refractivity Leica publishes for its
// instruments, reproduced by the formula, not copied. An instrument with
// another reference (a 15 C, dry reference is about 2.6 ppm away) wants
// AtmosphericCorrection::Fixed with the value its own manual gives.
//
// Compared with the owner's NeuralSurvey engine (engine/src/corrections.rs,
// atmospheric_ppm): it uses 286.34 - 0.29525 P / (1 + 0.00366 t), which is the
// same dry-air term with a coefficient 0.01 low (0.29535 is what the IUGG
// refractivity gives at 658 nm) and no humidity term. At 20 C, 1013.25 hPa and
// 60 % the two differ by about 0.45 ppm (0.45 mm per km).
inline constexpr double kCarrierWavelengthMicrometres = 0.658;
inline constexpr double kReferenceTemperatureCelsius = 12.0;
inline constexpr double kReferencePressureHectopascals = 1013.25;
inline constexpr double kReferenceHumidityPercent = 60.0;
// Used when a setup states temperature and pressure but no humidity. The
// humidity term is under 1 ppm below 25 C, so a guess of 60 % costs at most a
// fraction of that - and the report says it was guessed.
inline constexpr double kAssumedHumidityPercent = 60.0;

// Saturation vapour pressure over water, hPa (Magnus-Tetens).
[[nodiscard]] double saturationVapourPressure(double temperatureCelsius);

// N_L of the IUGG 1999 formula for the carrier above.
[[nodiscard]] double groupRefractivity(double temperatureCelsius, double pressureHectopascals,
                                       double relativeHumidityPercent);

// ppm = N_ref - N_L: the first velocity correction, positive when the air is
// thinner than the reference (warm, low pressure) and the distance must grow.
[[nodiscard]] double atmosphericPpm(double temperatureCelsius, double pressureHectopascals,
                                    double relativeHumidityPercent);

// ---- Slope to horizontal with curvature and refraction -----------------------------------
//
// The reduction every total station applies (as the Leica TPS1200 and TS16
// manuals print it, and the textbooks derive it):
//
//   X  = S cos z            the measured vertical component
//   Y  = S sin z            the measured horizontal component
//   HD = Y - A X Y          A = (1 - k/2) / R
//   VD = X + B Y^2          B = (1 - k) / (2 R)
//
// A X Y is the shortening of the horizontal distance because the level
// surface at the target is not the instrument's horizon; B Y^2 is the
// curvature of that surface less the refraction of the line of sight. The
// height difference mark to mark is VD + HI - HT. NeuralSurvey applies B Y^2
// to the height and nothing to the distance: A X Y is 12.7 mm on a 1 km line
// at a 5 degree slope (the target's plumb line is tilted by Y / R against the
// instrument's), and zero on a level line.
struct SlopeReduction {
    double measuredHorizontal = 0.0; // Y
    double measuredVertical = 0.0;   // X
    double horizontalCorrection = 0.0; // -A X Y (0 without curvature and refraction)
    double verticalCorrection = 0.0;   // +B Y^2 (0 without curvature and refraction)
};

[[nodiscard]] SlopeReduction reduceSlope(double slopeDistance, double zenithAngle,
                                         bool curvatureAndRefraction, double refraction,
                                         double earthRadius);

// R / (R + h): a horizontal distance at mean height h above the datum, taken
// down to the datum.
[[nodiscard]] double heightReductionFactor(double meanHeight, double earthRadius);

// ---- Distributions for the outlier tests -------------------------------------------------

// z with P(Z <= z) = probability for a standard normal Z; probability in (0, 1).
[[nodiscard]] double normalQuantile(double probability);

// t with P(T <= t) = probability for Student's t with `degreesOfFreedom` >= 1;
// probability in (0.5, 1). Exact (Abramowitz & Stegun 26.7.3 and 26.7.4, the
// finite series in theta = atan(t / sqrt(nu))) up to 200 degrees of freedom,
// the Cornish-Fisher expansion A&S 26.7.5 above that, whose truncation error is
// below 1e-8 there.
[[nodiscard]] double studentTQuantile(double probability, std::size_t degreesOfFreedom);

// Critical value of Baarda's w for a two-sided test at `significance`:
// the normal quantile at 1 - significance / 2 (3.29 for 0.001).
[[nodiscard]] double baardaCritical(double significance);

// Critical value of Pope's tau for redundancy r >= 2 at `significance`:
//   tau_c = t sqrt(r / (r - 1 + t^2)),  t = t_{r-1}(1 - significance / 2).
// Absent for r < 2, where tau cannot exceed its own maximum sqrt(r) in any
// informative way.
[[nodiscard]] std::optional<double> tauCritical(double significance, std::size_t redundancy);

} // namespace katana::survey::detail

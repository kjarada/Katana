#!/usr/bin/env python3
# tools/check_ifc.py <file.ifc> [--allow-proxies]
#
# IfcOpenShell's judgement of an IFC file Katana wrote (docs/ifc.md,
# "Validation"): an implementation of the schema that is not Katana's, so that
# a mistake in the writer and the same mistake in Katana's reader cannot
# agree with each other. Run by the cli.ifc_* tests when the Python that
# configured the build can import ifcopenshell; `pip install ifcopenshell`
# (and pytest, which IfcOpenShell's EXPRESS rule executor needs) provides it.
#
# What is checked:
#   * the file opens, and its schema is IFC4X3_ADD2;
#   * ifcopenshell.validate: every attribute's type, cardinality and
#     enumeration, the inverse attributes, and - with pytest present - the
#     schema's EXPRESS WHERE rules and functions. Any finding fails;
#   * no IfcBuildingElementProxy, unless --allow-proxies: the export's
#     promise (include/katana/ifc/classification.hpp);
#   * every alignment's geometry, evaluated by IfcOpenShell's own geometry
#     kernel, against its business logic: each horizontal segment must start
#     where, and in the direction, the composite curve is at the distance the
#     segments before it add up to, and each vertical segment at the height
#     and grade the gradient curve has at its StartDistAlong. The tolerances
#     are 1 mm and 1e-6 rad, far inside what a surveyor sets out to, and far
#     outside a correct file's rounding.
#
# Prints one summary line and each problem; exits 1 when there is one.

import importlib.util
import math
import sys

POSITION_TOLERANCE = 0.001  # metres
DIRECTION_TOLERANCE = 1e-6  # radians


def angle_between(a, b):
    return abs(math.atan2(math.sin(a - b), math.cos(a - b)))


def evaluator(settings, curve):
    from ifcopenshell import ifcopenshell_wrapper as wrapper

    function = wrapper.map_shape(settings, curve.wrapped_data)
    return function, wrapper.function_item_evaluator(settings, function)


def nested(product, entity):
    """What `product` nests of class `entity`, in the nesting's order."""
    out = []
    for relation in product.IsNestedBy or []:
        out.extend(o for o in relation.RelatedObjects if o.is_a(entity))
    return out


def check_alignment(alignment, settings, problems):
    """The worst position difference found, in metres, or None when the
    alignment has no geometry to evaluate."""
    shape = alignment.Representation
    if shape is None:
        return None
    curves = {r.RepresentationIdentifier: r.Items[0] for r in shape.Representations if r.Items}
    horizontal_curve = curves.get("FootPrint") or curves.get("Axis")
    if horizontal_curve is not None and horizontal_curve.is_a("IfcGradientCurve"):
        horizontal_curve = horizontal_curve.BaseCurve
    if horizontal_curve is None:
        return None
    worst = 0.0
    name = alignment.Name or alignment.GlobalId

    layouts = nested(alignment, "IfcAlignmentHorizontal")
    if layouts:
        function, evaluate = evaluator(settings, horizontal_curve)
        along = 0.0
        for segment in nested(layouts[0], "IfcAlignmentSegment"):
            design = segment.DesignParameters
            matrix = evaluate.evaluate(min(along, function.length()))
            x, y = matrix[0][3], matrix[1][3]
            direction = math.atan2(matrix[1][0], matrix[0][0])
            sx, sy = design.StartPoint.Coordinates[:2]
            offset = math.hypot(x - sx, y - sy)
            worst = max(worst, offset)
            if offset > POSITION_TOLERANCE:
                problems.append(
                    f"alignment {name}: horizontal segment {design.StartTag or ''} at {along:.3f} "
                    f"starts {offset:.4f} m from its geometry")
            turn = angle_between(direction, design.StartDirection)
            if turn > DIRECTION_TOLERANCE:
                problems.append(
                    f"alignment {name}: horizontal segment {design.StartTag or ''} at {along:.3f} "
                    f"is {turn:.2e} rad off its geometry's direction")
            along += design.SegmentLength

    gradient = curves.get("Axis")
    verticals = nested(alignment, "IfcAlignmentVertical")
    if verticals and gradient is not None and gradient.is_a("IfcGradientCurve"):
        function, evaluate = evaluator(settings, gradient)
        for segment in nested(verticals[0], "IfcAlignmentSegment"):
            design = segment.DesignParameters
            matrix = evaluate.evaluate(min(design.StartDistAlong, function.length()))
            height = matrix[2][3]
            difference = abs(height - design.StartHeight)
            worst = max(worst, difference)
            if difference > POSITION_TOLERANCE:
                problems.append(
                    f"alignment {name}: vertical segment at {design.StartDistAlong:.3f} is "
                    f"{difference:.4f} m from its gradient curve's height")
    return worst


def main(argv):
    if len(argv) < 2:
        print(__doc__ or "usage: check_ifc.py <file.ifc> [--allow-proxies]", file=sys.stderr)
        return 2
    path = argv[1]
    allow_proxies = "--allow-proxies" in argv[2:]

    import ifcopenshell
    import ifcopenshell.geom
    import ifcopenshell.validate

    problems = []
    model = ifcopenshell.open(path)
    if model.schema_identifier != "IFC4X3_ADD2":
        problems.append(f"the schema is {model.schema_identifier}, not IFC4X3_ADD2")

    logger = ifcopenshell.validate.json_logger()
    express = importlib.util.find_spec("pytest") is not None
    ifcopenshell.validate.validate(path, logger, express_rules=express)
    for statement in logger.statements:
        problems.append(f"{statement.get('type')}: {statement.get('instance')}: "
                        f"{str(statement.get('message'))[:300]}")
    checked = "the schema and its EXPRESS rules" if express else \
        "the schema (not its EXPRESS rules: pytest is not installed)"

    proxies = model.by_type("IfcBuildingElementProxy")
    if proxies and not allow_proxies:
        problems.append(f"{len(proxies)} IfcBuildingElementProxy")

    settings = ifcopenshell.geom.settings()
    alignments = model.by_type("IfcAlignment")
    worst = 0.0
    evaluated = 0
    for alignment in alignments:
        result = check_alignment(alignment, settings, problems)
        if result is not None:
            evaluated += 1
            worst = max(worst, result)

    products = {}
    for product in model.by_type("IfcProduct"):
        products[product.is_a()] = products.get(product.is_a(), 0) + 1
    classes = ", ".join(f"{name} {count}" for name, count in sorted(products.items()))
    print(f"{path}: {model.schema_identifier}, {len(list(model))} instances; checked against "
          f"{checked}: {len(logger.statements)} findings; {evaluated} of {len(alignments)} "
          f"alignments evaluated, worst {worst * 1000.0:.3f} mm; {classes}")
    for problem in problems:
        print("  " + problem)
    print("ok" if not problems else f"{len(problems)} problems")
    return 0 if not problems else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))

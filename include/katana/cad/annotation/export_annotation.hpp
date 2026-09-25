#pragma once

// Annotation drawn out as plain shapes for a file that has no such entities
// (docs/annotation.md, "Exchange").
//
// A DXF file has no label - a label's words and place are worked out for a
// view, by the placer - and the DXF writer draws only an aligned dimension
// and a bare leader itself. The file can still carry what the sheet shows:
// the lines, the arrowheads, the callout frames and the text, at the scale
// the drawing is annotated at. That drawing is katana_cad's (the placer, the
// dimension and leader builders), which the DXF module may not see
// (tools/check_layering.cmake), so a front end draws it here and hands it to
// the writer (dxf/writer.hpp, ExportOptions::drawn): one drawing of an
// annotation, whether it goes to the screen, a sheet or a file.
//
// The shapes are the plain kinds every format has: a stroke is a Segment2 or
// an open Polyline2, a filled arrowhead, a dot and a callout frame a CLOSED
// Polyline2 (the model cannot fill one; the outline is the shape), and each
// line of text a single-line, bottom-left TextGeometry at its model height.
// A mask is not drawn: a file's reader has no ground to paint it in.

#include <map>
#include <vector>

#include "katana/cad/annotation/drawing.hpp"
#include "katana/cad/dimension_draw.hpp"
#include "katana/entity/entity.hpp"
#include "katana/entity/model.hpp"

namespace katana::cad::annotation {

// By annotation entity: the shapes it is drawn as. An entry with no shapes is
// an annotation that draws nothing at this scale - a label the placer found
// no room for, or one whose target is gone.
using DrawnAnnotation = std::map<katana::entity::EntityId, std::vector<katana::entity::Geometry>>;

// Every label, leader and dimension of a kind other than Aligned in `model`,
// drawn at 1 : `scale` as the plan view draws them: labels placed together,
// kept out of each other and of the drawing (labelKeepOut), in label id
// order. Aligned dimensions are left to the writer, which has always drawn
// them. `measure` is the width the text is taken to have; the estimated one
// is what a file with no fonts is judged by.
[[nodiscard]] DrawnAnnotation drawAnnotationForExport(const katana::entity::Model& model,
                                                      double scale,
                                                      const TextMeasure& measure = estimatedMeasure());

// A drawing's pieces as plain shapes (above).
[[nodiscard]] std::vector<katana::entity::Geometry> plainShapes(const Drawing& drawing);
[[nodiscard]] std::vector<katana::entity::Geometry> plainShapes(const DimensionDrawing& drawing);

} // namespace katana::cad::annotation

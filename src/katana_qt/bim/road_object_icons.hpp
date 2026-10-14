#pragma once

// The icons of the BIM menu's Road Objects section (docs/road_objects.md,
// "In the window"), painted here rather than in icons.cpp so the road
// objects' drawings live with their workbench: icons.cpp's paintIcon hands
// these values here, the painter already on the 24-unit grid.

#include <QColor>

class QPainter;

namespace katana::qt {

// icons.hpp's, declared here rather than included so that icons.cpp - also
// compiled into katana_make_icons, the contact sheet - reaches this header by
// its own directory alone.
enum class Icon;

// Paints `which` - Icon::RoadBarrier to Icon::RoadObjects - on the 24-unit
// grid `painter` is set to, in the application's stroke (icons.cpp's 1.7
// units, round caps and joins): the object neutral, what makes it that
// object in the accent. False for an
// icon that is not one of these, which it leaves unpainted.
bool paintRoadObjectIcon(Icon which, QPainter& painter, const QColor& neutral,
                         const QColor& accent);

} // namespace katana::qt

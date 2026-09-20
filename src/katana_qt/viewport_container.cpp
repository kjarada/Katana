#include "viewport_container.hpp"

#include <QPainter>
#include <QResizeEvent>

namespace katana::qt {

namespace {

// Thickness of the border drawn round the active cell. Two pixels, inside the
// cell, so the tiling arithmetic is untouched by it.
constexpr int kActiveBorder = 2;

} // namespace

QWidget* ViewportContainer::Cell::widget() const
{
    if (plan) {
        return plan.get();
    }
    if (render) {
        return render.get();
    }
    return section.get();
}

ViewportContainer::ViewportContainer(katana::cad::Document& document, QWidget* parent)
    : QWidget(parent), document_(document)
{
    setAttribute(Qt::WA_OpaquePaintEvent, true);
    rebuildWidgets();
}

ViewContext ViewportContainer::contextFor() const
{
    ViewContext context;
    context.document = &document_;
    context.surfaces = surfaces_;
    context.options = options_;
    return context;
}

void ViewportContainer::buildCellWidget(std::size_t index)
{
    Cell cell;
    katana::cad::ViewportCell* model = &layout_.cell(index);

    switch (model->kind) {
    case katana::cad::ViewKind::Plan: {
        cell.plan = std::make_unique<ViewportWidget>(document_, this);
        ViewportWidget* plan = cell.plan.get();
        plan->setReferenceData(reference_);
        plan->setTool(tool_);
        plan->setGridVisible(gridVisible_);
        plan->setSnapEnabled(snapEnabled_);
        plan->setSnapModes(snapModes_);
        plan->onPrompt = [this](const QString& text) {
            if (onPrompt) {
                onPrompt(text);
            }
        };
        plan->onError = [this](const QString& text) {
            if (onError) {
                onError(text);
            }
        };
        plan->onCursorMoved = [this](const katana::geometry::Point2& world,
                                     const std::optional<katana::cad::SnapResult>& snap) {
            if (onCursorMoved) {
                onCursorMoved(world, snap);
            }
        };
        plan->installEventFilter(this);
        plan->onToolChanged = [this](Tool tool) {
            tool_ = tool;
            if (onToolChanged) {
                onToolChanged(tool);
            }
        };
        break;
    }
    case katana::cad::ViewKind::Model3D:
    case katana::cad::ViewKind::Elevation: {
        cell.render = std::make_unique<RenderViewWidget>(contextFor(), model, this);
        RenderViewWidget* view = cell.render.get();
        view->onActivated = [this, index] { activate(index); };
        view->onStatus = [this](const QString& text) {
            if (onStatus) {
                onStatus(text);
            }
        };
        break;
    }
    case katana::cad::ViewKind::Section: {
        cell.section = std::make_unique<SectionViewWidget>(this);
        SectionViewWidget* view = cell.section.get();
        view->onActivated = [this, index] { activate(index); };
        view->onStatus = [this](const QString& text) {
            if (onStatus) {
                onStatus(text);
            }
        };
        break;
    }
    }

    if (QWidget* widget = cell.widget()) {
        widget->show();
    }
    cells_[index] = std::move(cell);
}

void ViewportContainer::rebuildWidgets()
{
    // A Section cell's contents are the expensive thing to lose across a
    // layout change, so it is carried over when one survives.
    std::optional<katana::cad::Section> keptSection;
    for (const Cell& cell : cells_) {
        if (cell.section && cell.section->hasSection()) {
            keptSection = cell.section->section();
            break;
        }
    }

    cells_.clear();
    cells_.resize(layout_.size());
    for (std::size_t i = 0; i < layout_.size(); ++i) {
        buildCellWidget(i);
    }
    if (keptSection.has_value()) {
        for (Cell& cell : cells_) {
            if (cell.section) {
                cell.section->setSection(*keptSection);
                break;
            }
        }
    }
    applyGeometry();
    update();
}

void ViewportContainer::applyGeometry()
{
    layout_.setPixelSize(width(), height());
    for (std::size_t i = 0; i < cells_.size(); ++i) {
        const auto rect = layout_.pixelRect(i);
        if (!rect) {
            continue;
        }
        if (QWidget* widget = cells_[i].widget()) {
            widget->setGeometry(static_cast<int>(rect->x), static_cast<int>(rect->y),
                                static_cast<int>(rect->width), static_cast<int>(rect->height));
        }
    }
}

void ViewportContainer::resizeEvent(QResizeEvent* event)
{
    QWidget::resizeEvent(event);
    applyGeometry();
}

void ViewportContainer::paintEvent(QPaintEvent* /*event*/)
{
    // Children cover everything except the active-cell border, so this only
    // draws that. Which view is active decides where a command's clicks go, so
    // it has to be visible.
    QPainter painter(this);
    painter.fillRect(rect(), QColor(18, 18, 22));
    if (cells_.size() < 2) {
        return;
    }
    const auto rectResult = layout_.pixelRect(layout_.activeIndex());
    if (!rectResult) {
        return;
    }
    QPen pen(QColor(90, 150, 230), kActiveBorder);
    painter.setPen(pen);
    painter.drawRect(QRectF(rectResult->x + kActiveBorder * 0.5,
                            rectResult->y + kActiveBorder * 0.5,
                            rectResult->width - kActiveBorder,
                            rectResult->height - kActiveBorder));
}

void ViewportContainer::activate(std::size_t index)
{
    if (index == layout_.activeIndex()) {
        return;
    }
    layout_.setActiveIndex(index);
    update();
    if (onActiveChanged) {
        onActiveChanged();
    }
}

void ViewportContainer::setLayoutKind(katana::cad::LayoutKind kind)
{
    if (kind == layout_.layout()) {
        return;
    }
    layout_.setLayout(kind);
    rebuildWidgets();
    if (onActiveChanged) {
        onActiveChanged();
    }
}

void ViewportContainer::setActiveViewKind(katana::cad::ViewKind kind)
{
    const std::size_t index = layout_.activeIndex();
    if (auto status = layout_.setCellKind(index, kind); !status) {
        if (onStatus) {
            onStatus(QString::fromStdString(status.error().describe()));
        }
        return;
    }
    buildCellWidget(index);
    applyGeometry();
    update();
    if (onActiveChanged) {
        onActiveChanged();
    }
}

katana::cad::ViewKind ViewportContainer::activeViewKind() const
{
    return layout_.active().kind;
}

ViewportWidget* ViewportContainer::activePlanView()
{
    const std::size_t active = layout_.activeIndex();
    if (active < cells_.size() && cells_[active].plan) {
        return cells_[active].plan.get();
    }
    for (Cell& cell : cells_) {
        if (cell.plan) {
            return cell.plan.get();
        }
    }
    return nullptr;
}

RenderViewWidget* ViewportContainer::activeRenderView()
{
    const std::size_t active = layout_.activeIndex();
    if (active < cells_.size() && cells_[active].render) {
        return cells_[active].render.get();
    }
    for (Cell& cell : cells_) {
        if (cell.render) {
            return cell.render.get();
        }
    }
    return nullptr;
}

SectionViewWidget* ViewportContainer::activeSectionView()
{
    const std::size_t active = layout_.activeIndex();
    if (active < cells_.size() && cells_[active].section) {
        return cells_[active].section.get();
    }
    for (Cell& cell : cells_) {
        if (cell.section) {
            return cell.section.get();
        }
    }
    return nullptr;
}

std::vector<ViewportWidget*> ViewportContainer::planViews()
{
    std::vector<ViewportWidget*> out;
    for (Cell& cell : cells_) {
        if (cell.plan) {
            out.push_back(cell.plan.get());
        }
    }
    return out;
}

void ViewportContainer::setReferenceData(katana::interop::ReferenceData* reference)
{
    reference_ = reference;
    for (Cell& cell : cells_) {
        if (cell.plan) {
            cell.plan->setReferenceData(reference);
        }
    }
}

void ViewportContainer::setSurfaces(const std::vector<katana::cad::SceneSurface>* surfaces)
{
    surfaces_ = surfaces;
    for (Cell& cell : cells_) {
        if (cell.render) {
            cell.render->setContext(contextFor());
        }
    }
}

void ViewportContainer::setSceneOptions(const katana::cad::SceneOptions& options)
{
    options_ = options;
    for (Cell& cell : cells_) {
        if (cell.render) {
            cell.render->setContext(contextFor());
        }
    }
}

bool ViewportContainer::showSection(katana::cad::Section section)
{
    if (SectionViewWidget* view = activeSectionView()) {
        view->setSection(std::move(section));
        return true;
    }
    // No section cell yet. Turn the active one into one rather than silently
    // computing a section nobody can see; if the layout is a single plan view,
    // split it first so the drawing is not replaced.
    if (cells_.size() == 1) {
        setLayoutKind(katana::cad::LayoutKind::SplitHorizontal);
        layout_.setActiveIndex(1);
    }
    setActiveViewKind(katana::cad::ViewKind::Section);
    if (SectionViewWidget* view = activeSectionView()) {
        view->setSection(std::move(section));
        return true;
    }
    return false;
}

void ViewportContainer::refreshAll()
{
    for (Cell& cell : cells_) {
        if (cell.plan) {
            cell.plan->update();
        }
        if (cell.render) {
            cell.render->invalidateScene();
        }
        if (cell.section) {
            cell.section->update();
        }
    }
    update();
}

void ViewportContainer::invalidateReferenceCache()
{
    for (Cell& cell : cells_) {
        if (cell.plan) {
            cell.plan->invalidateReferenceCache();
        }
        if (cell.render) {
            cell.render->invalidateScene();
        }
    }
}

void ViewportContainer::setTool(Tool tool)
{
    tool_ = tool;
    for (Cell& cell : cells_) {
        if (cell.plan) {
            cell.plan->setTool(tool);
        }
    }
}

void ViewportContainer::setGridVisible(bool visible)
{
    gridVisible_ = visible;
    options_.drawGrid = visible;
    for (Cell& cell : cells_) {
        if (cell.plan) {
            cell.plan->setGridVisible(visible);
        }
        if (cell.render) {
            cell.render->setContext(contextFor());
        }
    }
}

void ViewportContainer::setSnapEnabled(bool enabled)
{
    snapEnabled_ = enabled;
    for (Cell& cell : cells_) {
        if (cell.plan) {
            cell.plan->setSnapEnabled(enabled);
        }
    }
}

void ViewportContainer::setSnapModes(katana::cad::SnapModes modes)
{
    snapModes_ = modes;
    for (Cell& cell : cells_) {
        if (cell.plan) {
            cell.plan->setSnapModes(modes);
        }
    }
}

void ViewportContainer::cancel()
{
    for (Cell& cell : cells_) {
        if (cell.plan) {
            cell.plan->cancel();
        }
    }
}

void ViewportContainer::resetInteraction()
{
    // Every plan cell, not just the active one: the clicks collected in a
    // background viewport belong to the drawing that is going away just as much
    // as the ones in front.
    for (Cell& cell : cells_) {
        if (cell.plan) {
            cell.plan->resetInteraction();
        }
    }
}

void ViewportContainer::zoomExtents()
{
    const std::size_t active = layout_.activeIndex();
    if (active < cells_.size() && !cells_[active].plan) {
        zoomExtentsActive();
        return;
    }
    for (Cell& cell : cells_) {
        if (cell.plan) {
            cell.plan->zoomExtents();
        }
        if (cell.render) {
            cell.render->zoomExtents();
        }
    }
}

void ViewportContainer::zoomTo(const katana::geometry::Box2& bounds)
{
    for (Cell& cell : cells_) {
        if (cell.plan) {
            cell.plan->zoomTo(bounds);
        }
    }
}

bool ViewportContainer::eventFilter(QObject* watched, QEvent* event)
{
    if (event->type() == QEvent::MouseButtonPress) {
        for (std::size_t i = 0; i < cells_.size(); ++i) {
            if (cells_[i].plan.get() == watched) {
                activate(i);
                break;
            }
        }
    }
    return QWidget::eventFilter(watched, event);
}

void ViewportContainer::zoomExtentsActive()
{
    const std::size_t active = layout_.activeIndex();
    if (active >= cells_.size()) {
        return;
    }
    if (cells_[active].plan) {
        cells_[active].plan->zoomExtents();
    } else if (cells_[active].render) {
        cells_[active].render->zoomExtents();
    } else if (cells_[active].section) {
        cells_[active].section->zoomExtents();
    }
}

} // namespace katana::qt

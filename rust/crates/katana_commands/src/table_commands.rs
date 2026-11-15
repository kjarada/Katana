//! Create, update and delete for every NamedTable in the model: linetypes,
//! dimension styles, hatch patterns, alignments, styles and the annotation
//! tables. Declared in entity_commands.rs, as the C++ declares them in
//! entity_commands.hpp.
//!
//! ONE implementation, parameterised by a policy per table. There used to be
//! four hand-written copies of the same three classes (and a fifth was about
//! to be written for styles), each thirty lines that differed only in the
//! noun, the table and the guards - which is the "second way of doing
//! something" CLAUDE.md says to collapse. What genuinely differs between the
//! tables is in `TableItem`:
//!
//!   * the item that may not be deleted (continuous, Standard, none);
//!   * an update that may not be made (the "none" hatch given a fill);
//!   * who still uses an item, so that a deletion is refused NAMING the holder
//!     rather than leaving a layer to draw solid with nothing to say why;
//!   * how to repoint those holders, which is what a rename is (`Repointable`).
//!
//! The guard and the repoint are two readings of the same references, so a
//! rename ENDS by asking the guard whether anything still names the old item:
//! if the two ever disagree it fails loudly instead of leaving an entity
//! pointing at a style that no longer exists. The guard itself reads
//! entity::table_usage - the one pass the managers' "Used" column and purge
//! read too - so a refusal says how many hold the item and names the first,
//! and no manager can call an item unused that a delete would then refuse.
//!
//! Command names ("CreateLinetype", "DeleteStyle" ...) are unchanged, since a
//! history shows them.

use katana_core::error::{Error, ErrorCode, Status, make_error};
use katana_entity::annotation::{
    DEFAULT_TEXT_STYLE_NAME, LabelRulePolicy, LabelStylePolicy, TextStylePolicy,
};
use katana_entity::table_usage::table_usage;
use katana_entity::tables::{
    AlignmentPolicy, CONTINUOUS_LINETYPE, DEFAULT_DIMENSION_STYLE_NAME, DimensionStylePolicy,
    HatchPatternPolicy, LinetypePolicy, NO_HATCH, StylePolicy,
};
use katana_entity::{
    Alignment, DimensionStyle, Entity, EntityId, Geometry, HatchPattern, LabelRule, LabelStyle,
    Layer, Linetype, Model, Named, NamedTable, NamedTablePolicy, Style, TableUsage, TextStyle,
    UsageOptions, Users, Validate,
};

use crate::command::{Command, CommandContext, CommandPtr};
use crate::entity_commands::TableItems;

// Repointing helpers for the renames below. Each walks one table and
// rewrites the named field of every holder of `from`, through that table's
// own update/replace so that validation and change notification happen
// exactly as they would for a user edit. The field is given by an accessor,
// as the C++ gives it by a member pointer.
fn repoint_layers(
    model: &mut Model,
    member: fn(&mut Layer) -> &mut String,
    from: &str,
    to: &str,
) -> Status {
    for name in model.layers.names() {
        let Some(current) = model.layers.find(&name) else {
            continue;
        };
        let mut updated = current.clone();
        if member(&mut updated).as_str() != from {
            continue;
        }
        *member(&mut updated) = String::from(to);
        model.layers.update(&updated)?;
    }
    Ok(())
}

fn repoint_styles(
    model: &mut Model,
    member: fn(&mut Style) -> &mut String,
    from: &str,
    to: &str,
) -> Status {
    for mut updated in model.styles.all() {
        if member(&mut updated).as_str() != from {
            continue;
        }
        *member(&mut updated) = String::from(to);
        model.styles.update(&updated)?;
    }
    Ok(())
}

fn layer_linetype(layer: &mut Layer) -> &mut String {
    &mut layer.linetype
}

fn style_linetype(style: &mut Style) -> &mut String {
    &mut style.linetype
}

fn repoint_entity_styles(model: &mut Model, from: &str, to: &str) -> Status {
    // Collected first: for_each may not mutate the database underneath itself.
    let mut wearers = Vec::new();
    model.entities.for_each(|entity| {
        if entity.style == from {
            wearers.push(entity.clone());
        }
    });
    for mut entity in wearers {
        entity.style = String::from(to);
        model.entities.replace(entity)?;
    }
    Ok(())
}

// The refusal every guard gives: how many hold the item and the first of
// them, so "used by 418 entities, e.g. id=12" can be acted on where "an
// entity still uses that style" could not.
fn refuse_if_used(users: &Users, noun: &str) -> Status {
    if !users.used() {
        return Ok(());
    }
    Err(make_error(
        ErrorCode::CommandRejected,
        format!("that {noun} is still used"),
        users.describe(),
    ))
}

// Before-images of everything a merge or a rename repoints, so its undo puts
// back exactly the holders it moved - and not the ones that already named the
// target before it ran.
#[derive(Clone, Debug, Default)]
struct HolderImages {
    layers: Vec<Layer>,
    styles: Vec<Style>,
    entities: Vec<Entity>,
}

fn restore_holders(model: &mut Model, images: &HolderImages) -> Status {
    for layer in &images.layers {
        model.layers.update(layer)?;
    }
    for style in &images.styles {
        model.styles.update(style)?;
    }
    for entity in &images.entities {
        model.entities.replace(entity.clone())?;
    }
    Ok(())
}

/// The C++ `TablePolicy<T>`: what one table differs in.
trait TableItem: Named + Validate + Clone + Default + PartialEq + Send + 'static {
    type Policy: NamedTablePolicy<Self>;
    const NOUN: &'static str;
    const STEM: &'static str;
    fn table(model: &Model) -> &NamedTable<Self, Self::Policy>;
    fn table_mut(model: &mut Model) -> &mut NamedTable<Self, Self::Policy>;
    /// The C++ writes `item.name` directly; a generic Rust item has no field.
    fn set_name(&mut self, name: &str);
    fn deletable(_name: &str) -> Status {
        Ok(())
    }
    fn updatable(_after: &Self) -> Status {
        Ok(())
    }
    fn in_use(model: &Model, name: &str) -> Status;
}

/// The tables a rename or a merge repoints the holders of.
trait Repointable: TableItem {
    fn holders(model: &Model, name: &str) -> HolderImages;
    fn repoint(model: &mut Model, from: &str, to: &str) -> Status;
}

impl TableItem for Linetype {
    type Policy = LinetypePolicy;
    const NOUN: &'static str = "linetype";
    const STEM: &'static str = "Linetype";
    fn table(model: &Model) -> &NamedTable<Self, Self::Policy> {
        &model.linetypes
    }
    fn table_mut(model: &mut Model) -> &mut NamedTable<Self, Self::Policy> {
        &mut model.linetypes
    }
    fn set_name(&mut self, name: &str) {
        self.name = String::from(name);
    }
    fn deletable(name: &str) -> Status {
        if name == CONTINUOUS_LINETYPE {
            return Err(Error::new(
                ErrorCode::CommandRejected,
                "the continuous linetype cannot be deleted",
            ));
        }
        Ok(())
    }
    // The table's own rule, asked rather than restated: validate() said a
    // pattern on "continuous" was fine and execute() then refused it (audit
    // MOD-09).
    fn updatable(after: &Self) -> Status {
        <LinetypePolicy as NamedTablePolicy<Linetype>>::check_update(after)
    }
    // Held by the layers and styles naming it; the entities reaching it
    // through them are counted too, since that is what a person weighs.
    fn in_use(model: &Model, name: &str) -> Status {
        let usage = table_usage(model, UsageOptions::default());
        refuse_if_used(TableUsage::of(&usage.linetypes, name), Self::NOUN)
    }
}

impl Repointable for Linetype {
    fn holders(model: &Model, name: &str) -> HolderImages {
        let usage = table_usage(model, UsageOptions::default());
        let users = TableUsage::of(&usage.linetypes, name);
        let mut images = HolderImages::default();
        for layer in &users.layers {
            if let Some(found) = model.layers.find(layer) {
                images.layers.push(found.clone());
            }
        }
        for style in &users.styles {
            if let Some(found) = model.styles.find(style) {
                images.styles.push(found.clone());
            }
        }
        images
    }
    fn repoint(model: &mut Model, from: &str, to: &str) -> Status {
        repoint_layers(model, layer_linetype, from, to)?;
        repoint_styles(model, style_linetype, from, to)
    }
}

impl TableItem for DimensionStyle {
    type Policy = DimensionStylePolicy;
    const NOUN: &'static str = "dimension style";
    const STEM: &'static str = "DimensionStyle";
    fn table(model: &Model) -> &NamedTable<Self, Self::Policy> {
        &model.dimension_styles
    }
    fn table_mut(model: &mut Model) -> &mut NamedTable<Self, Self::Policy> {
        &mut model.dimension_styles
    }
    fn set_name(&mut self, name: &str) {
        self.name = String::from(name);
    }
    fn deletable(name: &str) -> Status {
        if name == DEFAULT_DIMENSION_STYLE_NAME {
            return Err(Error::new(
                ErrorCode::CommandRejected,
                "the default dimension style cannot be deleted",
            ));
        }
        Ok(())
    }
    fn in_use(model: &Model, name: &str) -> Status {
        // A layer left naming a deleted style falls back to the default and
        // its dimensions silently change size.
        for layer in model.layers.names() {
            if let Some(definition) = model.layers.find(&layer)
                && definition.dimension_style == name
            {
                return Err(make_error(
                    ErrorCode::CommandRejected,
                    "a layer still uses that dimension style",
                    format!("layer={layer}"),
                ));
            }
        }
        Ok(())
    }
}

impl TableItem for HatchPattern {
    type Policy = HatchPatternPolicy;
    const NOUN: &'static str = "hatch pattern";
    const STEM: &'static str = "HatchPattern";
    fn table(model: &Model) -> &NamedTable<Self, Self::Policy> {
        &model.hatch_patterns
    }
    fn table_mut(model: &mut Model) -> &mut NamedTable<Self, Self::Policy> {
        &mut model.hatch_patterns
    }
    fn set_name(&mut self, name: &str) {
        self.name = String::from(name);
    }
    fn deletable(name: &str) -> Status {
        if name == NO_HATCH {
            return Err(Error::new(
                ErrorCode::CommandRejected,
                "the \"none\" hatch pattern cannot be deleted",
            ));
        }
        Ok(())
    }
    fn updatable(after: &Self) -> Status {
        if after.name == NO_HATCH && !after.draws_nothing() {
            return Err(Error::new(
                ErrorCode::CommandRejected,
                "the \"none\" hatch pattern cannot be given a fill",
            ));
        }
        Ok(())
    }
    fn in_use(model: &Model, name: &str) -> Status {
        let usage = table_usage(model, UsageOptions::default());
        refuse_if_used(TableUsage::of(&usage.hatch_patterns, name), Self::NOUN)
    }
}

impl TableItem for Alignment {
    type Policy = AlignmentPolicy;
    const NOUN: &'static str = "alignment";
    const STEM: &'static str = "Alignment";
    fn table(model: &Model) -> &NamedTable<Self, Self::Policy> {
        &model.alignments
    }
    fn table_mut(model: &mut Model) -> &mut NamedTable<Self, Self::Policy> {
        &mut model.alignments
    }
    fn set_name(&mut self, name: &str) {
        self.name = String::from(name);
    }
    // Nothing in the model references an alignment by name yet - sections
    // are cut and shown, not stored. When profiles or corridors are stored
    // they will, and this is where the guard belongs.
    fn in_use(_model: &Model, _name: &str) -> Status {
        Ok(())
    }
}

impl TableItem for Style {
    type Policy = StylePolicy;
    const NOUN: &'static str = "style";
    const STEM: &'static str = "Style";
    fn table(model: &Model) -> &NamedTable<Self, Self::Policy> {
        &model.styles
    }
    fn table_mut(model: &mut Model) -> &mut NamedTable<Self, Self::Policy> {
        &mut model.styles
    }
    fn set_name(&mut self, name: &str) {
        self.name = String::from(name);
    }
    fn in_use(model: &Model, name: &str) -> Status {
        // An entity left naming a deleted style would draw ByLayer with
        // nothing to say why; the count and the first holder are given so
        // they can be found.
        let usage = table_usage(model, UsageOptions::default());
        refuse_if_used(TableUsage::of(&usage.styles, name), Self::NOUN)
    }
}

impl Repointable for Style {
    fn holders(model: &Model, name: &str) -> HolderImages {
        let usage = table_usage(model, UsageOptions { entity_ids: true });
        let mut images = HolderImages::default();
        for &id in &TableUsage::of(&usage.styles, name).entity_ids {
            if let Some(found) = model.entities.find(id) {
                images.entities.push(found.clone());
            }
        }
        images
    }
    fn repoint(model: &mut Model, from: &str, to: &str) -> Status {
        repoint_entity_styles(model, from, to)
    }
}

// The annotation tables (annotation.rs). Each guard names the first holder,
// the shape refuse_if_used gives: the entity id for an entity, the style or
// rule name for a table.
impl TableItem for TextStyle {
    type Policy = TextStylePolicy;
    const NOUN: &'static str = "text style";
    const STEM: &'static str = "TextStyle";
    fn table(model: &Model) -> &NamedTable<Self, Self::Policy> {
        &model.text_styles
    }
    fn table_mut(model: &mut Model) -> &mut NamedTable<Self, Self::Policy> {
        &mut model.text_styles
    }
    fn set_name(&mut self, name: &str) {
        self.name = String::from(name);
    }
    fn deletable(name: &str) -> Status {
        if name == DEFAULT_TEXT_STYLE_NAME {
            return Err(Error::new(
                ErrorCode::CommandRejected,
                "the default text style cannot be deleted",
            ));
        }
        Ok(())
    }
    // A text or a leader left naming a deleted style would fall back to the
    // default face and height with nothing to say why, and a label style to
    // Standard.
    fn in_use(model: &Model, name: &str) -> Status {
        let mut count: usize = 0;
        let mut first: EntityId = 0;
        model.entities.for_each(|entity| {
            let style = match &entity.geometry {
                Geometry::Text(text) => Some(text.style.as_str()),
                Geometry::Leader(leader) => Some(leader.style.as_str()),
                _ => None,
            };
            if style == Some(name) {
                if count == 0 {
                    first = entity.id;
                }
                count += 1;
            }
        });
        if count > 0 {
            return Err(make_error(
                ErrorCode::CommandRejected,
                "that text style is still used",
                format!("used by {count} entities, e.g. id={first}"),
            ));
        }
        let mut holder = String::new();
        model.label_styles.for_each(|style| {
            if holder.is_empty() && style.text_style == name {
                holder = style.name.clone();
            }
        });
        if !holder.is_empty() {
            return Err(make_error(
                ErrorCode::CommandRejected,
                "that text style is still used",
                format!("label style={holder}"),
            ));
        }
        Ok(())
    }
}

impl TableItem for LabelStyle {
    type Policy = LabelStylePolicy;
    const NOUN: &'static str = "label style";
    const STEM: &'static str = "LabelStyle";
    fn table(model: &Model) -> &NamedTable<Self, Self::Policy> {
        &model.label_styles
    }
    fn table_mut(model: &mut Model) -> &mut NamedTable<Self, Self::Policy> {
        &mut model.label_styles
    }
    fn set_name(&mut self, name: &str) {
        self.name = String::from(name);
    }
    // A label left naming a deleted style would draw nothing at all, and a
    // smart leader in it (docs/annotation.md, "Smart leaders") no note.
    fn in_use(model: &Model, name: &str) -> Status {
        let mut count: usize = 0;
        let mut first: EntityId = 0;
        let mut leaders: usize = 0;
        let mut first_leader: EntityId = 0;
        model.entities.for_each(|entity| match &entity.geometry {
            Geometry::Label(label) if label.style == name => {
                if count == 0 {
                    first = entity.id;
                }
                count += 1;
            }
            Geometry::Leader(leader) if leader.label_style == name => {
                if leaders == 0 {
                    first_leader = entity.id;
                }
                leaders += 1;
            }
            _ => {}
        });
        if count > 0 {
            return Err(make_error(
                ErrorCode::CommandRejected,
                "that label style is still used",
                format!("used by {count} labels, e.g. id={first}"),
            ));
        }
        if leaders > 0 {
            return Err(make_error(
                ErrorCode::CommandRejected,
                "that label style is still used",
                format!("used by {leaders} leaders, e.g. id={first_leader}"),
            ));
        }
        let mut holder = String::new();
        model.label_rules.for_each(|rule| {
            if holder.is_empty() && rule.label_style == name {
                holder = rule.name.clone();
            }
        });
        if !holder.is_empty() {
            return Err(make_error(
                ErrorCode::CommandRejected,
                "that label style is still used",
                format!("label rule={holder}"),
            ));
        }
        Ok(())
    }
}

impl TableItem for LabelRule {
    type Policy = LabelRulePolicy;
    const NOUN: &'static str = "label rule";
    const STEM: &'static str = "LabelRule";
    fn table(model: &Model) -> &NamedTable<Self, Self::Policy> {
        &model.label_rules
    }
    fn table_mut(model: &mut Model) -> &mut NamedTable<Self, Self::Policy> {
        &mut model.label_rules
    }
    fn set_name(&mut self, name: &str) {
        self.name = String::from(name);
    }
    // The labels a rule made name it only so that the rule can find them
    // again; they stay, as hand-placed labels would, and AUTOLABEL CLEAR
    // still removes them by the name.
    fn in_use(_model: &Model, _name: &str) -> Status {
        Ok(())
    }
}

fn command_name<T: TableItem>(verb: &str) -> String {
    format!("{verb}{}", T::STEM)
}

fn does_not_exist<T: TableItem>(name: &str) -> Error {
    make_error(
        ErrorCode::NotFound,
        format!("{} does not exist", T::NOUN),
        name,
    )
}

fn already_exists<T: TableItem>(name: &str) -> Error {
    make_error(
        ErrorCode::AlreadyExists,
        format!("{} already exists", T::NOUN),
        name,
    )
}

struct CreateItemCommand<T> {
    item: T,
    name: String,
}

impl<T: TableItem> CreateItemCommand<T> {
    fn new(item: T) -> Self {
        Self {
            item,
            name: command_name::<T>("Create"),
        }
    }
}

impl<T: TableItem> Command for CreateItemCommand<T> {
    fn name(&self) -> &str {
        &self.name
    }
    fn validate(&self, context: &CommandContext<'_>) -> Status {
        if T::table(context.model).contains(self.item.name()) {
            return Err(already_exists::<T>(self.item.name()));
        }
        katana_entity::validate(&self.item)
    }
    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        T::table_mut(context.model).add(self.item.clone())
    }
    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        T::table_mut(context.model)
            .remove(self.item.name())
            .map(|_| ())
    }
    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.execute(context)
    }
}

struct UpdateItemCommand<T> {
    after: T,
    before: T,
    name: String,
}

impl<T: TableItem> UpdateItemCommand<T> {
    fn new(item: T) -> Self {
        Self {
            after: item,
            before: T::default(),
            name: command_name::<T>("Update"),
        }
    }
}

impl<T: TableItem> Command for UpdateItemCommand<T> {
    fn name(&self) -> &str {
        &self.name
    }
    fn validate(&self, context: &CommandContext<'_>) -> Status {
        if !T::table(context.model).contains(self.after.name()) {
            return Err(does_not_exist::<T>(self.after.name()));
        }
        T::updatable(&self.after)?;
        katana_entity::validate(&self.after)
    }
    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        let Some(current) = T::table(context.model).find(self.after.name()) else {
            return Err(does_not_exist::<T>(self.after.name()));
        };
        self.before = current.clone(); // before-image, as every other edit command takes one
        T::table_mut(context.model).update(&self.after)
    }
    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        T::table_mut(context.model).update(&self.before)
    }
    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        T::table_mut(context.model).update(&self.after)
    }
}

struct DeleteItemCommand<T> {
    item_name: String,
    removed: T,
    name: String,
}

impl<T: TableItem> DeleteItemCommand<T> {
    fn new(name: String) -> Self {
        Self {
            item_name: name,
            removed: T::default(),
            name: command_name::<T>("Delete"),
        }
    }
}

impl<T: TableItem> Command for DeleteItemCommand<T> {
    fn name(&self) -> &str {
        &self.name
    }
    // A deletion discards the definition - a dash pattern, an alignment's PI
    // and PVI design - so it asks for the same confirmation DELETE_LAYER does
    // (audit MOD-08).
    fn is_destructive(&self) -> bool {
        true
    }
    fn validate(&self, context: &CommandContext<'_>) -> Status {
        T::deletable(&self.item_name)?;
        if !T::table(context.model).contains(&self.item_name) {
            return Err(does_not_exist::<T>(&self.item_name));
        }
        T::in_use(context.model, &self.item_name)
    }
    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.removed = T::table_mut(context.model).remove(&self.item_name)?;
        Ok(())
    }
    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        T::table_mut(context.model).add(self.removed.clone())
    }
    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.execute(context)
    }
}

// Remove, re-add under the new name, repoint every holder. NOT an update of
// the name field: a NamedTable is keyed by name, so a rename is a move, and
// remove+add keeps the table's own validation of the new name rather than a
// second copy of those rules here.
//
// Undo is NOT the reverse rename. The new name may already have holders the
// table does not know of - a style naming a library linestyle, entities
// wearing a style that is "not in the style table" - and repointing every
// holder of the new name back would move those too. So undo puts back the
// before-images of what execute() moved, as MergeItemCommand does.
struct RenameItemCommand<T> {
    from: String,
    to: String,
    name: String,
    holders: HolderImages,
    item: std::marker::PhantomData<T>,
}

impl<T: Repointable> RenameItemCommand<T> {
    fn new(from: String, to: String) -> Self {
        Self {
            from,
            to,
            name: command_name::<T>("Rename"),
            holders: HolderImages::default(),
            item: std::marker::PhantomData,
        }
    }

    // The item goes back under its old name first, so the holders put back
    // after it name something that exists at every step.
    fn roll_back(&self, model: &mut Model) -> Status {
        let mut item = T::table_mut(model).remove(&self.to)?;
        item.set_name(&self.from);
        T::table_mut(model).add(item)?;
        restore_holders(model, &self.holders)
    }
}

impl<T: Repointable> Command for RenameItemCommand<T> {
    fn name(&self) -> &str {
        &self.name
    }
    fn validate(&self, context: &CommandContext<'_>) -> Status {
        // A protected item is protected from being renamed away as much as
        // from being deleted: everything that resolves to it by name would
        // silently change what it draws.
        T::deletable(&self.from)?;
        let Some(current) = T::table(context.model).find(&self.from) else {
            return Err(does_not_exist::<T>(&self.from));
        };
        if T::table(context.model).contains(&self.to) {
            return Err(already_exists::<T>(&self.to));
        }
        let mut renamed = current.clone();
        renamed.set_name(&self.to);
        katana_entity::validate(&renamed)
    }
    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        // Taken before anything moves: these, and only these, are what undo
        // puts back.
        self.holders = T::holders(context.model, &self.from);
        let mut item = T::table_mut(context.model).remove(&self.from)?;
        item.set_name(&self.to);
        if let Err(error) = T::table_mut(context.model).add(item.clone()) {
            item.set_name(&self.from);
            let _ = T::table_mut(context.model).add(item);
            return Err(error);
        }
        let mut status = T::repoint(context.model, &self.from, &self.to);
        if status.is_ok() {
            // The guard and the repoint read the same references, so
            // anything still naming `from` is a bug to report, not a holder to
            // leave pointing at nothing.
            status = T::in_use(context.model, &self.from);
        }
        if let Err(error) = status {
            let _ = self.roll_back(context.model); // execute() leaves the model as it found it
            return Err(error);
        }
        Ok(())
    }
    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.roll_back(context.model)
    }
    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.execute(context)
    }
}

// Repoint every holder of `from` to `into`, then delete `from`: what a rename
// onto an existing name has to be, and why RenameItemCommand refuses one.
// One command, so one undo puts back the item AND every holder it moved.
struct MergeItemCommand<T> {
    from: String,
    into: String,
    name: String,
    holders: HolderImages,
    removed: T,
}

impl<T: Repointable> MergeItemCommand<T> {
    fn new(from: String, into: String) -> Self {
        Self {
            from,
            into,
            name: command_name::<T>("Merge"),
            holders: HolderImages::default(),
            removed: T::default(),
        }
    }

    fn roll_back(&self, model: &mut Model) -> Status {
        restore_holders(model, &self.holders)?;
        T::table_mut(model).add(self.removed.clone())
    }
}

impl<T: Repointable> Command for MergeItemCommand<T> {
    fn name(&self) -> &str {
        &self.name
    }
    // The merged item's own definition is discarded.
    fn is_destructive(&self) -> bool {
        true
    }
    fn validate(&self, context: &CommandContext<'_>) -> Status {
        if self.from == self.into {
            return Err(make_error(
                ErrorCode::InvalidArgument,
                format!("a {} cannot be merged into itself", T::NOUN),
                self.from.as_str(),
            ));
        }
        // Protected for the reason a rename is: everything that resolves to
        // it by name would silently change what it draws.
        T::deletable(&self.from)?;
        if !T::table(context.model).contains(&self.from) {
            return Err(does_not_exist::<T>(&self.from));
        }
        if !T::table(context.model).contains(&self.into) {
            return Err(make_error(
                ErrorCode::NotFound,
                format!("the {} to merge into does not exist", T::NOUN),
                self.into.as_str(),
            ));
        }
        Ok(())
    }
    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.holders = T::holders(context.model, &self.from);
        self.removed = T::table_mut(context.model).remove(&self.from)?;
        let mut status = T::repoint(context.model, &self.from, &self.into);
        if status.is_ok() {
            // The rename's own check: the guard and the repoint read the same
            // references, so anything still naming `from` is a bug to report,
            // not an entity to leave pointing at nothing.
            status = T::in_use(context.model, &self.from);
        }
        if let Err(error) = status {
            let _ = self.roll_back(context.model); // execute() leaves the model as it found it
            return Err(error);
        }
        Ok(())
    }
    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.roll_back(context.model)
    }
    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.execute(context)
    }
}

// A copy under a new name: the start of "a style like that one, but red".
struct DuplicateItemCommand<T> {
    from: String,
    to: String,
    name: String,
    copy: T,
}

impl<T: TableItem> DuplicateItemCommand<T> {
    fn new(from: String, to: String) -> Self {
        Self {
            from,
            to,
            name: command_name::<T>("Duplicate"),
            copy: T::default(),
        }
    }
}

impl<T: TableItem> Command for DuplicateItemCommand<T> {
    fn name(&self) -> &str {
        &self.name
    }
    fn validate(&self, context: &CommandContext<'_>) -> Status {
        let Some(source) = T::table(context.model).find(&self.from) else {
            return Err(does_not_exist::<T>(&self.from));
        };
        if T::table(context.model).contains(&self.to) {
            return Err(already_exists::<T>(&self.to));
        }
        let mut copy = source.clone();
        copy.set_name(&self.to);
        katana_entity::validate(&copy)
    }
    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        let Some(source) = T::table(context.model).find(&self.from) else {
            return Err(does_not_exist::<T>(&self.from));
        };
        self.copy = source.clone();
        self.copy.set_name(&self.to);
        T::table_mut(context.model).add(self.copy.clone())
    }
    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        T::table_mut(context.model).remove(&self.to).map(|_| ())
    }
    // The copy taken at execute(), not a fresh one: redo reproduces what
    // execute did even if `from` has been edited in between by a command that
    // has since been undone.
    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        T::table_mut(context.model).add(self.copy.clone())
    }
}

fn update_if_changed<T: TableItem>(model: &Model, item: T) -> Option<CommandPtr> {
    // Saving a form nobody edited is not an edit: no undo step, and the
    // project is not marked modified by it.
    if let Some(current) = T::table(model).find(item.name())
        && *current == item
    {
        return None;
    }
    Some(Box::new(UpdateItemCommand::new(item)))
}

#[derive(Clone, Debug, Default)]
struct Removed {
    styles: Vec<Style>,
    linetypes: Vec<Linetype>,
    hatch_patterns: Vec<HatchPattern>,
}

// A purge: many unused items as ONE step. Its guard is one usage pass for the
// whole set - one DeleteItemCommand per item would be one pass over the
// entities per item - and it is judged as the set: a linetype named only by
// styles this purge also removes is free.
struct PurgeItemsCommand {
    items: TableItems,
    removed: Removed,
}

fn all_present<T: TableItem>(model: &Model, names: &[String]) -> Status {
    for name in names {
        T::deletable(name)?;
        if !T::table(model).contains(name) {
            return Err(does_not_exist::<T>(name));
        }
    }
    Ok(())
}

fn with_name(status: Status, name: &str) -> Status {
    status.map_err(|error| {
        make_error(
            error.code,
            error.message,
            format!("{name}: {}", error.context),
        )
    })
}

fn remove_all<T: Named + Clone, P: NamedTablePolicy<T>>(
    table: &mut NamedTable<T, P>,
    names: &[String],
    removed: &mut Vec<T>,
) -> Status {
    for name in names {
        removed.push(table.remove(name)?);
    }
    Ok(())
}

impl Command for PurgeItemsCommand {
    fn name(&self) -> &str {
        "PurgeTables"
    }
    fn is_destructive(&self) -> bool {
        true
    }
    fn validate(&self, context: &CommandContext<'_>) -> Status {
        let model: &Model = context.model;
        if self.items.empty() {
            // Never an empty undo step (the shape of audit QT-01).
            return Err(Error::new(
                ErrorCode::InvalidArgument,
                "there is nothing to purge",
            ));
        }
        all_present::<Style>(model, &self.items.styles)?;
        all_present::<Linetype>(model, &self.items.linetypes)?;
        all_present::<HatchPattern>(model, &self.items.hatch_patterns)?;
        let usage = table_usage(model, UsageOptions::default());
        for name in &self.items.styles {
            let status = refuse_if_used(TableUsage::of(&usage.styles, name), "style");
            if status.is_err() {
                return with_name(status, name);
            }
        }
        // An entity reaches a linetype or hatch only through a layer or a
        // style, and the styles going have no entities (checked above), so
        // what holds one is a layer, or a style that is staying.
        let held_by_what_stays = |users: &Users| {
            if !users.layers.is_empty() {
                return true;
            }
            users
                .styles
                .iter()
                .any(|style| !self.items.styles.contains(style))
        };
        for name in &self.items.linetypes {
            let users = TableUsage::of(&usage.linetypes, name);
            if held_by_what_stays(users) {
                return with_name(refuse_if_used(users, "linetype"), name);
            }
        }
        for name in &self.items.hatch_patterns {
            let users = TableUsage::of(&usage.hatch_patterns, name);
            if held_by_what_stays(users) {
                return with_name(refuse_if_used(users, "hatch pattern"), name);
            }
        }
        Ok(())
    }
    fn execute(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.removed = Removed::default();
        let mut status = remove_all(
            &mut context.model.styles,
            &self.items.styles,
            &mut self.removed.styles,
        );
        if status.is_ok() {
            status = remove_all(
                &mut context.model.linetypes,
                &self.items.linetypes,
                &mut self.removed.linetypes,
            );
        }
        if status.is_ok() {
            status = remove_all(
                &mut context.model.hatch_patterns,
                &self.items.hatch_patterns,
                &mut self.removed.hatch_patterns,
            );
        }
        if let Err(error) = status {
            let _ = self.undo(context);
            return Err(error);
        }
        Ok(())
    }
    fn undo(&mut self, context: &mut CommandContext<'_>) -> Status {
        for pattern in &self.removed.hatch_patterns {
            context.model.hatch_patterns.add(pattern.clone())?;
        }
        for linetype in &self.removed.linetypes {
            context.model.linetypes.add(linetype.clone())?;
        }
        for style in &self.removed.styles {
            context.model.styles.add(style.clone())?;
        }
        self.removed = Removed::default();
        Ok(())
    }
    fn redo(&mut self, context: &mut CommandContext<'_>) -> Status {
        self.execute(context)
    }
}

// ---- the factories ------------------------------------------------------------------

// ---- linetypes -------------------------------------------------------------------

pub fn create_linetype(linetype: Linetype) -> CommandPtr {
    Box::new(CreateItemCommand::new(linetype))
}

pub fn update_linetype(linetype: Linetype) -> CommandPtr {
    Box::new(UpdateItemCommand::new(linetype))
}

/// None when `linetype` equals the stored one - saving an unedited form is
/// not an edit and must push no undo step. A missing name still gets a
/// command, whose validate() says NotFound.
pub fn update_linetype_if_changed(model: &Model, linetype: Linetype) -> Option<CommandPtr> {
    update_if_changed(model, linetype)
}

/// Refuses while any layer or style still names it, and refuses "continuous".
pub fn delete_linetype(name: impl Into<String>) -> CommandPtr {
    Box::new(DeleteItemCommand::<Linetype>::new(name.into()))
}

/// Renames it and repoints every layer and style that named it, as one undo
/// step. Refuses an existing target name - two linetypes of the same name
/// would have to be merged, and picking one definition would change how
/// everything wearing the other draws. merge_linetype is that, said on purpose.
pub fn rename_linetype(from: impl Into<String>, to: impl Into<String>) -> CommandPtr {
    Box::new(RenameItemCommand::<Linetype>::new(from.into(), to.into()))
}

/// Repoints every layer and style naming `from` to `into`, then deletes
/// `from`: one undo step restores both. Refuses "continuous" as `from`, and
/// an `into` that is not a linetype in the model (a library linestyle is not
/// one: the commands cannot see the library).
pub fn merge_linetype(from: impl Into<String>, into: impl Into<String>) -> CommandPtr {
    Box::new(MergeItemCommand::<Linetype>::new(from.into(), into.into()))
}

/// A copy of `from` named `to`.
pub fn duplicate_linetype(from: impl Into<String>, to: impl Into<String>) -> CommandPtr {
    Box::new(DuplicateItemCommand::<Linetype>::new(
        from.into(),
        to.into(),
    ))
}

// ---- dimension styles ------------------------------------------------------------

pub fn create_dimension_style(style: DimensionStyle) -> CommandPtr {
    Box::new(CreateItemCommand::new(style))
}

pub fn update_dimension_style(style: DimensionStyle) -> CommandPtr {
    Box::new(UpdateItemCommand::new(style))
}

/// Refuses while any layer still names it, and refuses "Standard".
pub fn delete_dimension_style(name: impl Into<String>) -> CommandPtr {
    Box::new(DeleteItemCommand::<DimensionStyle>::new(name.into()))
}

// ---- hatch patterns --------------------------------------------------------------

pub fn create_hatch_pattern(pattern: HatchPattern) -> CommandPtr {
    Box::new(CreateItemCommand::new(pattern))
}

pub fn update_hatch_pattern(pattern: HatchPattern) -> CommandPtr {
    Box::new(UpdateItemCommand::new(pattern))
}

/// Refuses while any layer or style still names it, and refuses "none".
pub fn delete_hatch_pattern(name: impl Into<String>) -> CommandPtr {
    Box::new(DeleteItemCommand::<HatchPattern>::new(name.into()))
}

// ---- styles ----------------------------------------------------------------------
// A style is how an entity that is not ByLayer is drawn: linetype, weight,
// colour, hatch and - for a point - its symbol. Delete refuses while any
// entity still names it. Names are what a library linestyle arrives as.

pub fn create_style(style: Style) -> CommandPtr {
    Box::new(CreateItemCommand::new(style))
}

pub fn update_style(style: Style) -> CommandPtr {
    Box::new(UpdateItemCommand::new(style))
}

/// None when `style` equals the stored one; see update_linetype_if_changed.
pub fn update_style_if_changed(model: &Model, style: Style) -> Option<CommandPtr> {
    update_if_changed(model, style)
}

pub fn delete_style(name: impl Into<String>) -> CommandPtr {
    Box::new(DeleteItemCommand::<Style>::new(name.into()))
}

/// Renames it and repoints every entity that wore it, as one undo step. An
/// archive import names a style after a linestyle ("TOPO Natural Surface
/// Point"), which is the first thing a user wants to shorten.
pub fn rename_style(from: impl Into<String>, to: impl Into<String>) -> CommandPtr {
    Box::new(RenameItemCommand::<Style>::new(from.into(), to.into()))
}

/// Moves every entity wearing `from` onto `into`, then deletes `from`, as one
/// undo step - two styles that should have been one. Refuses a missing
/// `from` or `into`, and `from == into`.
pub fn merge_style(from: impl Into<String>, into: impl Into<String>) -> CommandPtr {
    Box::new(MergeItemCommand::<Style>::new(from.into(), into.into()))
}

/// A copy of `from` named `to`.
pub fn duplicate_style(from: impl Into<String>, to: impl Into<String>) -> CommandPtr {
    Box::new(DuplicateItemCommand::<Style>::new(from.into(), to.into()))
}

// ---- purge -----------------------------------------------------------------------

/// Deletes them all as ONE undo step ("PurgeTables"). Refuses an empty set -
/// an empty undo step is a defect (audit QT-01's shape) - a protected or
/// missing name, and anything still used once the set is gone, naming it and
/// its first holder. cad::purge_command is what finds the set.
pub fn purge_table_items(items: TableItems) -> CommandPtr {
    Box::new(PurgeItemsCommand {
        items,
        removed: Removed::default(),
    })
}

// ---- the annotation tables ---------------------------------------------------------
// Text styles, label styles and auto-label rules, through the same create /
// update / delete as the tables above. Deleting a text style a text, leader or
// label style still names, or a label style a label or rule still names, is
// refused naming the holder; "Standard" text cannot be deleted.

pub fn create_text_style(style: TextStyle) -> CommandPtr {
    Box::new(CreateItemCommand::new(style))
}

pub fn update_text_style(style: TextStyle) -> CommandPtr {
    Box::new(UpdateItemCommand::new(style))
}

pub fn delete_text_style(name: impl Into<String>) -> CommandPtr {
    Box::new(DeleteItemCommand::<TextStyle>::new(name.into()))
}

pub fn create_label_style(style: LabelStyle) -> CommandPtr {
    Box::new(CreateItemCommand::new(style))
}

pub fn update_label_style(style: LabelStyle) -> CommandPtr {
    Box::new(UpdateItemCommand::new(style))
}

pub fn delete_label_style(name: impl Into<String>) -> CommandPtr {
    Box::new(DeleteItemCommand::<LabelStyle>::new(name.into()))
}

pub fn create_label_rule(rule: LabelRule) -> CommandPtr {
    Box::new(CreateItemCommand::new(rule))
}

pub fn update_label_rule(rule: LabelRule) -> CommandPtr {
    Box::new(UpdateItemCommand::new(rule))
}

pub fn delete_label_rule(name: impl Into<String>) -> CommandPtr {
    Box::new(DeleteItemCommand::<LabelRule>::new(name.into()))
}

// ---- alignments ------------------------------------------------------------------
// Create and update validate the definition by solving it, so an alignment
// that cannot be built is refused naming its PI rather than stored.

pub fn create_alignment(alignment: Alignment) -> CommandPtr {
    Box::new(CreateItemCommand::new(alignment))
}

pub fn update_alignment(alignment: Alignment) -> CommandPtr {
    Box::new(UpdateItemCommand::new(alignment))
}

pub fn delete_alignment(name: impl Into<String>) -> CommandPtr {
    Box::new(DeleteItemCommand::<Alignment>::new(name.into()))
}

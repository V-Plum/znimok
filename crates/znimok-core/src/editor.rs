//! The single handler of document commands and queries (`znimok-core::apply` in PLAN §5.4).
//! GUI, CLI, MCP and tests all go through [`Editor::apply`] and [`Editor::query`].

use crate::command::*;
#[cfg(test)]
use crate::history::MergeKey;
use crate::history::{History, Weigh};
use crate::hit;
use crate::model::*;

/// What undo restores (LH `EdSnap`): marks, selection, crop, recipe, current original and
/// group names. Name and metadata are edited in the Meta panel and are not undo steps (as in LH).
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    objects: Vec<Object>,
    selection: Vec<ObjectId>,
    crop: Option<IRect>,
    recipe: Recipe,
    source: BankId,
    group_names: std::collections::BTreeMap<GroupId, String>,
    /// A video's cuts and in/out: undone together with the marks (ZK-144).
    timeline: Option<Timeline>,
}

impl Weigh for Snapshot {
    fn weigh(&self) -> usize {
        let per_object: usize = self
            .objects
            .iter()
            .map(|o| {
                192 + o.name.as_ref().map_or(0, String::len)
                    + match &o.data {
                        Data::Pen { points, .. } => points.len() * 8,
                        Data::Text { text, .. } => text.len(),
                        _ => 0,
                    }
            })
            .sum();
        128 + per_object
            + self.selection.len() * 4
            + self
                .group_names
                .values()
                .map(|n| n.len() + 16)
                .sum::<usize>()
    }
}

pub struct Editor {
    pub doc: Document,
    /// Selected ids; the first one is the primary selection (inspector shows it).
    selection: Vec<ObjectId>,
    history: History<Snapshot>,
    revision: u64,
    saved_revision: u64,
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        Self {
            doc,
            selection: Vec::new(),
            history: History::default(),
            revision: 0,
            saved_revision: 0,
        }
    }

    pub fn with_history_limits(doc: Document, max_steps: usize, max_bytes: usize) -> Self {
        Self {
            history: History::new(max_steps, max_bytes),
            ..Self::new(doc)
        }
    }

    pub fn selection(&self) -> &[ObjectId] {
        &self.selection
    }

    pub fn is_dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// Call after the document was saved to the library.
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }

    /// Parses and applies a JSON command.
    pub fn apply_json(&mut self, json: &str) -> Result<Applied, CoreError> {
        let cmd = parse_command(json)?;
        self.apply(cmd)
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            objects: self.doc.objects.clone(),
            selection: self.selection.clone(),
            crop: self.doc.crop,
            recipe: self.doc.recipe,
            source: self.doc.source,
            group_names: self.doc.group_names.clone(),
            timeline: self.doc.timeline.clone(),
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.doc.objects = s.objects;
        self.selection = s.selection;
        self.doc.crop = s.crop;
        self.doc.recipe = s.recipe;
        self.doc.source = s.source;
        self.doc.group_names = s.group_names;
        self.doc.timeline = s.timeline;
        let max_id = self.doc.objects.iter().map(|o| o.id).max().unwrap_or(0);
        self.doc.next_id = self.doc.next_id.max(max_id + 1);
    }

    fn everything_changed(&self) -> Vec<Change> {
        vec![
            Change::Objects {
                ids: self.doc.objects.iter().map(|o| o.id).collect(),
            },
            Change::Order,
            Change::Selection,
            Change::Crop,
            Change::Recipe,
            Change::Timeline,
            Change::History,
        ]
    }

    pub fn apply(&mut self, cmd: Command) -> Result<Applied, CoreError> {
        if !cmd.is_document() {
            return Err(CoreError::NotInCore(cmd.name()));
        }
        match cmd {
            Command::Undo => {
                let cur = self.snapshot();
                let Some(prev) = self.history.undo(cur) else {
                    return Ok(Applied::default());
                };
                self.restore(prev);
                self.revision += 1;
                return Ok(Applied {
                    changes: self.everything_changed(),
                    created: None,
                });
            }
            Command::Redo => {
                let cur = self.snapshot();
                let Some(next) = self.history.redo(cur) else {
                    return Ok(Applied::default());
                };
                self.restore(next);
                self.revision += 1;
                return Ok(Applied {
                    changes: self.everything_changed(),
                    created: None,
                });
            }
            Command::Select { ids, add } => {
                self.check_ids(&ids)?;
                if !add {
                    self.selection.clear();
                }
                for id in ids {
                    if !self.selection.contains(&id) {
                        self.selection.push(id);
                    }
                }
                self.history.break_series();
                return Ok(Applied {
                    changes: vec![Change::Selection],
                    created: None,
                });
            }
            Command::SelectAll => {
                self.selection = self
                    .doc
                    .objects
                    .iter()
                    .filter(|o| !o.hidden)
                    .map(|o| o.id)
                    .collect();
                self.history.break_series();
                return Ok(Applied {
                    changes: vec![Change::Selection],
                    created: None,
                });
            }
            Command::ClearSelection => {
                self.selection.clear();
                self.history.break_series();
                return Ok(Applied {
                    changes: vec![Change::Selection],
                    created: None,
                });
            }
            Command::SetName { name } => {
                if self.doc.name == name {
                    return Ok(Applied::default());
                }
                self.doc.name = name;
                self.revision += 1;
                return Ok(Applied {
                    changes: vec![Change::Meta],
                    created: None,
                });
            }
            Command::SetMeta { meta } => {
                if self.doc.meta == meta {
                    return Ok(Applied::default());
                }
                self.doc.meta = meta;
                self.revision += 1;
                return Ok(Applied {
                    changes: vec![Change::Meta],
                    created: None,
                });
            }
            _ => {}
        }

        let merge = match &cmd {
            Command::AddObject { merge, .. }
            | Command::UpdateObjects { merge, .. }
            | Command::MoveObjects { merge, .. }
            | Command::ResizeObject { merge, .. }
            | Command::SetTone { merge, .. }
            | Command::SetTimeline { merge, .. } => merge.clone(),
            _ => None,
        };
        let before = self.snapshot();
        let mut applied = self.mutate(cmd)?;
        if self.snapshot() == before {
            // Nothing really changed: no undo step, no dirty flag (LH `EdGroupApply`).
            return Ok(Applied::default());
        }
        if merge.is_none() {
            self.history.break_series();
        }
        self.history.record(before, merge);
        self.revision += 1;
        applied.changes.push(Change::History);
        Ok(applied)
    }

    fn check_ids(&self, ids: &[ObjectId]) -> Result<(), CoreError> {
        match ids.iter().find(|id| self.doc.index_of(**id).is_none()) {
            Some(id) => Err(CoreError::UnknownObject(*id)),
            None => Ok(()),
        }
    }

    /// Ids plus every other member of their selection groups, in z-order.
    fn with_groups(&self, ids: &[ObjectId]) -> Vec<ObjectId> {
        let groups: Vec<GroupId> = ids
            .iter()
            .filter_map(|id| self.doc.get(*id))
            .map(|o| o.group)
            .filter(|g| *g != 0)
            .collect();
        self.doc
            .objects
            .iter()
            .filter(|o| ids.contains(&o.id) || (o.group != 0 && groups.contains(&o.group)))
            .map(|o| o.id)
            .collect()
    }

    fn mutate(&mut self, cmd: Command) -> Result<Applied, CoreError> {
        let changed = |ids: Vec<ObjectId>| Applied {
            changes: vec![Change::Objects { ids }],
            created: None,
        };
        match cmd {
            Command::AddObject {
                mut object, select, ..
            } => {
                if let Data::Image { bank } = object.data
                    && bank as usize >= self.doc.banks.len()
                {
                    return Err(CoreError::Invalid(format!(
                        "image bank {bank} does not exist"
                    )));
                }
                if let Data::Counter {
                    seq, group, start, ..
                } = &mut object.data
                {
                    *seq = self.doc.next_counter_seq();
                    if let Some(s) = self.doc.counter_group_start(*group) {
                        *start = s;
                    }
                }
                object.style.alpha = object.style.alpha.clamp(10, 100);
                object.style.alpha2 = object.style.alpha2.clamp(10, 100);
                if object.group != 0 && !self.doc.objects.iter().any(|o| o.group == object.group) {
                    object.group = 0;
                }
                // Counters of one numbering group are one group of marks too (ZK-169): a new
                // counter joins the group its numbering-mates are in, and the second counter of a
                // numbering group makes that group — in this one undo step.
                if object.group == 0
                    && let Data::Counter {
                        group: numbering, ..
                    } = object.data
                {
                    let mates: Vec<usize> = self
                        .doc
                        .objects
                        .iter()
                        .enumerate()
                        .filter(|(_, o)| {
                            matches!(o.data, Data::Counter { group, .. } if group == numbering)
                        })
                        .map(|(i, _)| i)
                        .collect();
                    if !mates.is_empty() {
                        let g = mates
                            .iter()
                            .map(|i| self.doc.objects[*i].group)
                            .find(|g| *g != 0)
                            .unwrap_or_else(|| self.doc.next_group_id());
                        for i in mates {
                            if self.doc.objects[i].group == 0 {
                                self.doc.objects[i].group = g;
                            }
                        }
                        object.group = g;
                    }
                }
                let i = self.doc.push(object);
                let id = self.doc.objects[i].id;
                let mut changes = vec![Change::Added { id }];
                if self.doc.objects[i].group != 0 {
                    self.doc.compact_groups();
                    changes.push(Change::Order);
                }
                if select {
                    self.selection = vec![id];
                    changes.push(Change::Selection);
                }
                Ok(Applied {
                    changes,
                    created: Some(id),
                })
            }
            Command::UpdateObjects { ids, patch, .. } => {
                self.check_ids(&ids)?;
                for id in &ids {
                    let o = self.doc.get_mut(*id).expect("checked");
                    apply_patch(o, &patch)?;
                    o.sanitize();
                }
                Ok(changed(ids))
            }
            Command::DeleteObjects { ids } => {
                self.check_ids(&ids)?;
                self.doc.objects.retain(|o| !ids.contains(&o.id));
                self.selection.retain(|id| !ids.contains(id));
                let used: Vec<GroupId> = self.doc.objects.iter().map(|o| o.group).collect();
                self.doc.group_names.retain(|g, _| used.contains(g));
                Ok(Applied {
                    changes: vec![Change::Removed { ids }, Change::Selection],
                    created: None,
                })
            }
            Command::MoveObjects { ids, dx, dy, .. } => {
                self.check_ids(&ids)?;
                for id in &ids {
                    self.doc.get_mut(*id).expect("checked").translate(dx, dy);
                }
                Ok(changed(ids))
            }
            Command::ResizeObject {
                id,
                handle,
                orig,
                dx,
                dy,
                keep_ratio,
                from_centre,
                ..
            } => {
                let (orig, dx, dy) = (orig.clamped(), clamp_coord(dx), clamp_coord(dy));
                let o = self.doc.get_mut(id).ok_or(CoreError::UnknownObject(id))?;
                if handle >= hit::handles(o).len() {
                    return Err(CoreError::Invalid(format!(
                        "object {id} has no handle {handle}"
                    )));
                }
                hit::resize_with(
                    o,
                    handle,
                    orig,
                    dx,
                    dy,
                    hit::ResizeMods {
                        keep_ratio,
                        from_centre,
                    },
                );
                o.sanitize();
                Ok(changed(vec![id]))
            }
            Command::Group { ids } => {
                self.check_ids(&ids)?;
                let all = self.with_groups(&ids);
                if all.len() < 2 {
                    return Err(CoreError::Invalid(
                        "a group needs at least two marks".into(),
                    ));
                }
                let g = self.doc.next_group_id();
                for id in &all {
                    self.doc.get_mut(*id).expect("checked").group = g;
                }
                self.doc.compact_groups();
                let used: Vec<GroupId> = self.doc.objects.iter().map(|o| o.group).collect();
                self.doc.group_names.retain(|k, _| used.contains(k));
                Ok(Applied {
                    changes: vec![Change::Objects { ids: all }, Change::Order],
                    created: None,
                })
            }
            Command::Ungroup { ids } => {
                self.check_ids(&ids)?;
                let all = self.with_groups(&ids);
                let groups: Vec<GroupId> = all
                    .iter()
                    .filter_map(|id| self.doc.get(*id))
                    .map(|o| o.group)
                    .collect();
                for id in &all {
                    self.doc.get_mut(*id).expect("checked").group = 0;
                }
                self.doc.group_names.retain(|g, _| !groups.contains(g));
                Ok(changed(all))
            }
            Command::RenameGroup { group, name } => {
                if !self
                    .doc
                    .objects
                    .iter()
                    .any(|o| o.group == group && group != 0)
                {
                    return Err(CoreError::Invalid(format!("no group {group}")));
                }
                if name.trim().is_empty() {
                    self.doc.group_names.remove(&group);
                } else {
                    self.doc.group_names.insert(group, name);
                }
                Ok(Applied {
                    changes: vec![Change::Meta],
                    created: None,
                })
            }
            Command::Restack { order, groups } => {
                let n = self.doc.objects.len();
                let mut seen = std::collections::HashSet::new();
                if order.len() != n || groups.len() != n || !order.iter().all(|id| seen.insert(*id))
                {
                    return Err(CoreError::Invalid(
                        "restack needs every mark exactly once, with a group for each".into(),
                    ));
                }
                self.check_ids(&order)?;
                // A group of one is no group.
                let mut count: std::collections::HashMap<GroupId, usize> =
                    std::collections::HashMap::new();
                for g in groups.iter().filter(|g| **g != 0) {
                    *count.entry(*g).or_default() += 1;
                }
                let mut old = std::mem::take(&mut self.doc.objects);
                let mut objects = Vec::with_capacity(n);
                for (id, g) in order.iter().zip(&groups) {
                    let i = old.iter().position(|o| o.id == *id).expect("checked");
                    let mut o = old.swap_remove(i);
                    o.group = if count.get(g).copied().unwrap_or(0) >= 2 {
                        *g
                    } else {
                        0
                    };
                    objects.push(o);
                }
                self.doc.objects = objects;
                self.doc.compact_groups();
                let used: Vec<GroupId> = self.doc.objects.iter().map(|o| o.group).collect();
                self.doc.group_names.retain(|k, _| used.contains(k));
                Ok(Applied {
                    changes: vec![Change::Objects { ids: order }, Change::Order],
                    created: None,
                })
            }
            Command::Arrange { ids, to } => {
                self.check_ids(&ids)?;
                let sel = self.with_groups(&ids);
                // Work on units: a selection group (adjacent by invariant) moves as one block,
                // and a single step forward/backward jumps over a whole neighbouring group.
                self.doc.compact_groups();
                let mut units: Vec<Vec<Object>> = Vec::new();
                for o in std::mem::take(&mut self.doc.objects) {
                    match units.last_mut() {
                        Some(u) if o.group != 0 && u[0].group == o.group => u.push(o),
                        _ => units.push(vec![o]),
                    }
                }
                let is_sel = |u: &Vec<Object>| sel.contains(&u[0].id);
                let units = match to {
                    Arrange::Front => {
                        let (s, mut rest): (Vec<_>, Vec<_>) = units.into_iter().partition(is_sel);
                        rest.extend(s);
                        rest
                    }
                    Arrange::Back => {
                        let (mut s, rest): (Vec<_>, Vec<_>) = units.into_iter().partition(is_sel);
                        s.extend(rest);
                        s
                    }
                    Arrange::Forward => {
                        let mut v = units;
                        for i in (0..v.len().saturating_sub(1)).rev() {
                            if is_sel(&v[i]) && !is_sel(&v[i + 1]) {
                                v.swap(i, i + 1);
                            }
                        }
                        v
                    }
                    Arrange::Backward => {
                        let mut v = units;
                        for i in 1..v.len() {
                            if is_sel(&v[i]) && !is_sel(&v[i - 1]) {
                                v.swap(i, i - 1);
                            }
                        }
                        v
                    }
                };
                self.doc.objects = units.into_iter().flatten().collect();
                Ok(Applied {
                    changes: vec![Change::Order],
                    created: None,
                })
            }
            Command::Align { ids, edge } => {
                self.check_ids(&ids)?;
                let bounds: Vec<IRect> = ids
                    .iter()
                    .map(|id| self.doc.get(*id).expect("checked").bounds())
                    .collect();
                let target = if ids.len() == 1 {
                    self.doc.frame()
                } else {
                    bounds
                        .iter()
                        .copied()
                        .reduce(IRect::union)
                        .expect("non-empty")
                };
                for (id, b) in ids.iter().zip(&bounds) {
                    let (dx, dy) = match edge {
                        AlignEdge::Left => (target.x - b.x, 0),
                        AlignEdge::Right => (target.right() - b.right(), 0),
                        AlignEdge::HCenter => ((2 * target.x + target.w - 2 * b.x - b.w) / 2, 0),
                        AlignEdge::Top => (0, target.y - b.y),
                        AlignEdge::Bottom => (0, target.bottom() - b.bottom()),
                        AlignEdge::VCenter => (0, (2 * target.y + target.h - 2 * b.y - b.h) / 2),
                    };
                    self.doc.get_mut(*id).expect("checked").translate(dx, dy);
                }
                Ok(changed(ids))
            }
            Command::Distribute { ids, axis } => {
                self.check_ids(&ids)?;
                if ids.len() < 3 {
                    return Err(CoreError::Invalid(
                        "distribution needs at least three marks".into(),
                    ));
                }
                let mut items: Vec<(ObjectId, IRect)> = ids
                    .iter()
                    .map(|id| (*id, self.doc.get(*id).expect("checked").bounds()))
                    .collect();
                let horizontal = axis == Axis::Horizontal;
                // i64: many marks near the coordinate limit would overflow i32 sums.
                let start = |r: &IRect| i64::from(if horizontal { r.x } else { r.y });
                let size = |r: &IRect| i64::from(if horizontal { r.w } else { r.h });
                items.sort_by_key(|(_, r)| start(r));
                let first = start(&items[0].1);
                let last_end = items
                    .iter()
                    .map(|(_, r)| start(r) + size(r))
                    .max()
                    .expect("non-empty");
                let total: i64 = items.iter().map(|(_, r)| size(r)).sum();
                let n = items.len() as i64;
                let gap2 = 2 * (last_end - first - total); // doubled to keep halves exact
                let mut pos2 = 2 * first;
                for (k, (id, r)) in items.iter().enumerate() {
                    let target = pos2.div_euclid(2);
                    let d = if k == 0 || k as i64 == n - 1 {
                        0
                    } else {
                        target - start(r)
                    };
                    let d = d.clamp(i64::from(-COORD_LIMIT), i64::from(COORD_LIMIT)) as i32;
                    let o = self.doc.get_mut(*id).expect("checked");
                    match axis {
                        Axis::Horizontal => o.translate(d, 0),
                        Axis::Vertical => o.translate(0, d),
                    }
                    pos2 += 2 * size(r) + gap2 / (n - 1);
                }
                Ok(changed(ids))
            }
            Command::SetCounterStart { group, start } => {
                if self.doc.counter_group_start(group).is_none() {
                    return Err(CoreError::Invalid(format!("no counter group {group}")));
                }
                self.doc.set_counter_group_start(group, start);
                let ids = self
                    .doc
                    .objects
                    .iter()
                    .filter(|o| matches!(o.data, Data::Counter { group: g, .. } if g == group))
                    .map(|o| o.id)
                    .collect();
                Ok(changed(ids))
            }
            Command::SetTimeline { timeline, .. } => {
                let Some(cur) = &self.doc.timeline else {
                    return Err(CoreError::Invalid("not a video document".into()));
                };
                if !timeline.is_valid() || timeline.frames() != cur.frames() {
                    return Err(CoreError::Invalid(format!(
                        "the timeline must cover 0..{} in contiguous non-empty parts, with 0 ≤ in < out ≤ {}",
                        cur.frames(),
                        cur.frames()
                    )));
                }
                self.doc.timeline = Some(timeline);
                Ok(Applied {
                    changes: vec![Change::Timeline],
                    created: None,
                })
            }
            Command::SetCrop { rect } => {
                self.doc.crop = match rect {
                    None => None,
                    Some(r) => {
                        let (w, h) = self.doc.image_size();
                        let r = r.normalized();
                        let x0 = r.x.clamp(0, w as i32);
                        let y0 = r.y.clamp(0, h as i32);
                        let x1 = r.right().clamp(0, w as i32);
                        let y1 = r.bottom().clamp(0, h as i32);
                        if x1 - x0 < 1 || y1 - y0 < 1 {
                            return Err(CoreError::Invalid(
                                "the crop is empty or outside the picture".into(),
                            ));
                        }
                        let c = IRect::new(x0, y0, x1 - x0, y1 - y0);
                        if c == IRect::new(0, 0, w as i32, h as i32) {
                            None
                        } else {
                            Some(c)
                        }
                    }
                };
                Ok(Applied {
                    changes: vec![Change::Crop],
                    created: None,
                })
            }
            Command::SetTone {
                exposure,
                gamma,
                contrast,
                ..
            } => {
                let mut r = self.doc.recipe;
                if let Some(v) = exposure {
                    r.exposure = v;
                }
                if let Some(v) = gamma {
                    r.gamma = v;
                }
                if let Some(v) = contrast {
                    r.contrast = v;
                }
                self.doc.recipe = r.clamped();
                Ok(Applied {
                    changes: vec![Change::Recipe],
                    created: None,
                })
            }
            Command::ResetTone => {
                let r = self.doc.recipe;
                self.doc.recipe = Recipe {
                    exposure: 0.0,
                    gamma: 1.0,
                    contrast: 0,
                    ..r
                };
                Ok(Applied {
                    changes: vec![Change::Recipe],
                    created: None,
                })
            }
            Command::Rotate { quarters } => {
                self.doc.rotate_quarters(quarters);
                Ok(Applied {
                    changes: vec![
                        Change::Recipe,
                        Change::Crop,
                        Change::Objects {
                            ids: self.doc.objects.iter().map(|o| o.id).collect(),
                        },
                    ],
                    created: None,
                })
            }
            Command::ResizeImage {
                width,
                height,
                scale_text,
            } => {
                crate::pixels::resize_image(&mut self.doc, width, height, scale_text)
                    .map_err(CoreError::Invalid)?;
                Ok(Applied {
                    changes: vec![
                        Change::Recipe,
                        Change::Crop,
                        Change::Objects {
                            ids: self.doc.objects.iter().map(|o| o.id).collect(),
                        },
                    ],
                    created: None,
                })
            }
            Command::ResizeCanvas { rect, fill } => {
                crate::pixels::resize_canvas(&mut self.doc, rect, fill)
                    .map_err(CoreError::Invalid)?;
                Ok(Applied {
                    changes: vec![
                        Change::Recipe,
                        Change::Crop,
                        Change::Objects {
                            ids: self.doc.objects.iter().map(|o| o.id).collect(),
                        },
                    ],
                    created: None,
                })
            }
            Command::Mirror | Command::MirrorVertical => {
                if matches!(cmd, Command::MirrorVertical) {
                    self.doc.rotate_quarters(2);
                }
                self.doc.mirror_horizontal();
                Ok(Applied {
                    changes: vec![
                        Change::Recipe,
                        Change::Crop,
                        Change::Objects {
                            ids: self.doc.objects.iter().map(|o| o.id).collect(),
                        },
                    ],
                    created: None,
                })
            }
            other => Err(CoreError::NotInCore(other.name())),
        }
    }

    // ---- queries

    fn info(&self, i: usize) -> ObjectInfo {
        let o = &self.doc.objects[i];
        ObjectInfo {
            index: i,
            bounds: o.bounds(),
            counter_number: self.doc.counter_number(i),
            object: o.clone(),
        }
    }

    pub fn query(&self, q: &Query) -> QueryResult {
        match q {
            Query::GetDocument => {
                let d = &self.doc;
                QueryResult::Document {
                    document: DocumentInfo {
                        id: d.id.to_string(),
                        name: d.name.clone(),
                        meta: d.meta.clone(),
                        image_size: d.image_size(),
                        frame: d.frame(),
                        crop: d.crop,
                        recipe: d.recipe,
                        shot_scale: d.shot_scale,
                        object_count: d.objects.len(),
                        banks: d.banks.iter().map(|b| (b.width, b.height)).collect(),
                        source: d.source,
                        group_names: d.group_names.iter().map(|(k, v)| (*k, v.clone())).collect(),
                    },
                }
            }
            Query::ListObjects => QueryResult::Objects {
                objects: (0..self.doc.objects.len()).map(|i| self.info(i)).collect(),
            },
            Query::GetObject { id } => QueryResult::Object {
                object: self.doc.index_of(*id).map(|i| self.info(i)),
            },
            Query::GetSelection => QueryResult::Selection {
                ids: self.selection.clone(),
            },
            Query::GetState => QueryResult::State {
                state: EditorState {
                    schema_version: SCHEMA_VERSION,
                    can_undo: self.history.can_undo(),
                    can_redo: self.history.can_redo(),
                    undo_steps: self.history.undo_len(),
                    redo_steps: self.history.redo_len(),
                    dirty: self.is_dirty(),
                    selection: self.selection.clone(),
                },
            },
            Query::HitTest { x, y, px_per_doc } => QueryResult::Hit {
                id: hit::pick(&self.doc, (*x, *y), px_per_doc.unwrap_or(1.0))
                    .map(|i| self.doc.objects[i].id),
            },
            Query::ObjectsInRect { rect } => QueryResult::Objects {
                objects: hit::pick_in_rect(&self.doc, *rect)
                    .into_iter()
                    .map(|i| self.info(i))
                    .collect(),
            },
        }
    }

    /// Parses a JSON query and answers with JSON.
    pub fn query_json(&self, json: &str) -> Result<String, CoreError> {
        let q = parse_query(json)?;
        serde_json::to_string(&self.query(&q)).map_err(|e| CoreError::Invalid(e.to_string()))
    }
}

fn apply_patch(o: &mut Object, p: &ObjectPatch) -> Result<(), CoreError> {
    if let Some(d) = &p.data {
        if d.kind() != o.kind() {
            return Err(CoreError::Invalid(format!(
                "object {} is {:?}; data of {:?} cannot replace it",
                o.id,
                o.kind(),
                d.kind()
            )));
        }
        let keep_seq = match (&o.data, d) {
            (Data::Counter { seq, .. }, Data::Counter { .. }) => Some(*seq),
            _ => None,
        };
        o.data = d.clone();
        if let (Some(s), Data::Counter { seq, .. }) = (keep_seq, &mut o.data) {
            *seq = s;
        }
    }
    if let Some(r) = p.rect {
        o.rect = if o.kind() == Kind::Line {
            r
        } else {
            r.normalized()
        };
        if let Data::Pen { points, .. } = &mut o.data {
            // A new box for a pen trail scales its points into it.
            let b = crate::model::Object::new(IRect::default(), Data::pen(points.clone())).bounds();
            let sx = if b.w > 0 {
                r.w as f64 / b.w as f64
            } else {
                1.0
            };
            let sy = if b.h > 0 {
                r.h as f64 / b.h as f64
            } else {
                1.0
            };
            for pt in points.iter_mut() {
                pt.0 = r.x + ((pt.0 - b.x) as f64 * sx).round() as i32;
                pt.1 = r.y + ((pt.1 - b.y) as f64 * sy).round() as i32;
            }
        }
    }
    if let Some(s) = &p.style {
        s.apply(&mut o.style);
    }
    if let Some(r) = p.rot
        && o.kind().can_rotate()
    {
        o.rot = r % 360;
    }
    if let Some(n) = &p.name {
        o.name = n.clone().filter(|s| !s.trim().is_empty());
    }
    if let Some(h) = p.hidden {
        o.hidden = h;
    }
    if let Data::Text {
        text,
        size,
        bold,
        italic,
        align,
        ..
    } = &mut o.data
    {
        if let Some(t) = &p.text {
            *text = t.clone();
        }
        if let Some(v) = p.size {
            *size = v.clamp(1, 1600);
        }
        if let Some(v) = p.bold {
            *bold = v;
        }
        if let Some(v) = p.italic {
            *italic = v;
        }
        if let Some(v) = p.align {
            *align = v;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> Editor {
        Editor::new(Document::from_raster(
            "t",
            Raster::solid(400, 300, Rgb::WHITE),
        ))
    }

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Object {
        Object::new(IRect::new(x, y, w, h), Data::Rect)
    }

    fn add(e: &mut Editor, o: Object) -> ObjectId {
        e.apply(Command::AddObject {
            object: o,
            select: false,
            merge: None,
        })
        .unwrap()
        .created
        .unwrap()
    }

    #[test]
    fn add_move_undo_redo_and_dirty() {
        let mut e = editor();
        assert!(!e.is_dirty());
        let id = add(&mut e, rect(10, 10, 50, 40));
        assert!(e.is_dirty());
        e.mark_saved();
        e.apply(Command::MoveObjects {
            ids: vec![id],
            dx: 5,
            dy: 0,
            merge: None,
        })
        .unwrap();
        assert_eq!(e.doc.get(id).unwrap().rect.x, 15);
        e.apply(Command::Undo).unwrap();
        assert_eq!(e.doc.get(id).unwrap().rect.x, 10);
        e.apply(Command::Undo).unwrap();
        assert!(e.doc.objects.is_empty());
        e.apply(Command::Redo).unwrap();
        e.apply(Command::Redo).unwrap();
        assert_eq!(e.doc.get(id).unwrap().rect.x, 15);
    }

    #[test]
    fn drawing_a_mark_is_one_step() {
        let mut e = editor();
        let key = Some(MergeKey::Drag { id: 9 });
        let id = e
            .apply(Command::AddObject {
                object: rect(10, 10, 1, 1),
                select: true,
                merge: key.clone(),
            })
            .unwrap()
            .created
            .unwrap();
        for w in [20, 40, 80] {
            let patch = ObjectPatch {
                rect: Some(IRect::new(10, 10, w, w / 2)),
                ..Default::default()
            };
            e.apply(Command::UpdateObjects {
                ids: vec![id],
                patch,
                merge: key.clone(),
            })
            .unwrap();
        }
        let QueryResult::State { state } = e.query(&Query::GetState) else {
            unreachable!()
        };
        assert_eq!(state.undo_steps, 1);
        e.apply(Command::Undo).unwrap();
        assert!(e.doc.objects.is_empty());
    }

    #[test]
    fn nudges_merge_and_noops_do_not_record() {
        let mut e = editor();
        let id = add(&mut e, rect(10, 10, 50, 40));
        for _ in 0..5 {
            e.apply(Command::MoveObjects {
                ids: vec![id],
                dx: 1,
                dy: 0,
                merge: Some(MergeKey::Nudge),
            })
            .unwrap();
        }
        let QueryResult::State { state } = e.query(&Query::GetState) else {
            unreachable!()
        };
        assert_eq!(state.undo_steps, 2); // add + one nudge series
        // A patch that changes nothing is not an undo step.
        let r = e
            .apply(Command::UpdateObjects {
                ids: vec![id],
                patch: ObjectPatch {
                    hidden: Some(false),
                    ..Default::default()
                },
                merge: None,
            })
            .unwrap();
        assert!(r.is_empty());
        let QueryResult::State { state } = e.query(&Query::GetState) else {
            unreachable!()
        };
        assert_eq!(state.undo_steps, 2);
        e.apply(Command::Undo).unwrap();
        assert_eq!(e.doc.get(id).unwrap().rect.x, 10);
    }

    #[test]
    fn selection_is_not_an_undo_step_but_is_restored() {
        let mut e = editor();
        let a = add(&mut e, rect(0, 0, 10, 10));
        e.apply(Command::Select {
            ids: vec![a],
            add: false,
        })
        .unwrap();
        e.apply(Command::DeleteObjects { ids: vec![a] }).unwrap();
        assert!(e.selection().is_empty());
        e.apply(Command::Undo).unwrap();
        assert_eq!(e.selection(), &[a]);
    }

    #[test]
    fn counters_get_sequence_and_group_start() {
        let mut e = editor();
        let c = |start| {
            Object::new(
                IRect::new(0, 0, 36, 36),
                Data::Counter {
                    seq: 99,
                    group: 1,
                    start,
                    shape: CounterShape::Circle,
                },
            )
        };
        let a = add(&mut e, c(5));
        let b = add(&mut e, c(1)); // joins group 1 → start 5
        let QueryResult::Object { object: Some(info) } = e.query(&Query::GetObject { id: b })
        else {
            unreachable!()
        };
        assert_eq!(info.counter_number, Some(6));
        e.apply(Command::DeleteObjects { ids: vec![a] }).unwrap();
        let QueryResult::Object { object: Some(info) } = e.query(&Query::GetObject { id: b })
        else {
            unreachable!()
        };
        assert_eq!(info.counter_number, Some(5));
        e.apply(Command::SetCounterStart {
            group: 1,
            start: 10,
        })
        .unwrap();
        let QueryResult::Object { object: Some(info) } = e.query(&Query::GetObject { id: b })
        else {
            unreachable!()
        };
        assert_eq!(info.counter_number, Some(10));
    }

    #[test]
    fn counters_of_one_numbering_group_form_one_group_in_one_undo_step() {
        let mut e = Editor::new(Document::from_raster(
            "t",
            Raster::solid(200, 200, Rgb::WHITE),
        ));
        let counter = |numbering: u32| {
            Object::new(
                IRect::new(10, 10, 28, 28),
                Data::Counter {
                    seq: 0,
                    group: numbering,
                    start: 1,
                    shape: crate::model::CounterShape::Circle,
                },
            )
        };
        let add = |e: &mut Editor, o: Object| {
            e.apply(Command::AddObject {
                object: o,
                select: true,
                merge: None,
            })
            .unwrap()
            .created
            .unwrap()
        };
        let a = add(&mut e, counter(1));
        assert_eq!(e.doc.get(a).unwrap().group, 0, "alone, it is no group yet");
        let b = add(&mut e, counter(1));
        let g = e.doc.get(a).unwrap().group;
        assert!(g != 0 && e.doc.get(b).unwrap().group == g);
        assert_eq!(e.selection(), &[b], "only the new counter is selected");
        let c = add(&mut e, counter(1));
        assert_eq!(e.doc.get(c).unwrap().group, g);
        let other = add(&mut e, counter(2));
        assert_eq!(
            e.doc.get(other).unwrap().group,
            0,
            "another numbering group stays apart"
        );
        e.apply(Command::Undo).unwrap();
        e.apply(Command::Undo).unwrap();
        e.apply(Command::Undo).unwrap();
        assert_eq!(
            e.doc.get(a).unwrap().group,
            0,
            "one undo takes the grouping back too"
        );
    }

    #[test]
    fn group_arrange_keeps_members_together() {
        let mut e = editor();
        let ids: Vec<_> = (0..5).map(|i| add(&mut e, rect(i * 10, 0, 5, 5))).collect();
        e.apply(Command::Group {
            ids: vec![ids[0], ids[2]],
        })
        .unwrap();
        let order: Vec<_> = e.doc.objects.iter().map(|o| o.id).collect();
        assert_eq!(order, [ids[1], ids[0], ids[2], ids[3], ids[4]]);
        // Raising one member raises the whole group.
        e.apply(Command::Arrange {
            ids: vec![ids[0]],
            to: Arrange::Front,
        })
        .unwrap();
        let order: Vec<_> = e.doc.objects.iter().map(|o| o.id).collect();
        assert_eq!(order, [ids[1], ids[3], ids[4], ids[0], ids[2]]);
        e.apply(Command::Arrange {
            ids: vec![ids[4]],
            to: Arrange::Forward,
        })
        .unwrap();
        let order: Vec<_> = e.doc.objects.iter().map(|o| o.id).collect();
        assert_eq!(order, [ids[1], ids[3], ids[0], ids[2], ids[4]]);
        e.apply(Command::Arrange {
            ids: vec![ids[4]],
            to: Arrange::Back,
        })
        .unwrap();
        assert_eq!(e.doc.objects[0].id, ids[4]);
        e.apply(Command::Ungroup { ids: vec![ids[2]] }).unwrap();
        assert!(e.doc.objects.iter().all(|o| o.group == 0));
    }

    #[test]
    fn restack_moves_and_groups_in_one_step() {
        let mut e = editor();
        let ids: Vec<ObjectId> = (0..4)
            .map(|i| add(&mut e, rect(10 * i, 10, 5, 5)))
            .collect();
        // New order (back to front) 3 0 1 2, with 0 and 2 in group 5, 1 alone in group 9.
        e.apply(Command::Restack {
            order: vec![ids[3], ids[0], ids[1], ids[2]],
            groups: vec![0, 5, 9, 5],
        })
        .unwrap();
        let got: Vec<(ObjectId, GroupId)> = e.doc.objects.iter().map(|o| (o.id, o.group)).collect();
        // The group gathers under its highest member; the lone "group" dissolves.
        assert_eq!(got, [(ids[3], 0), (ids[1], 0), (ids[0], 5), (ids[2], 5)]);
        e.apply(Command::Undo).unwrap();
        assert!(e.doc.objects.iter().all(|o| o.group == 0));
        assert!(
            e.apply(Command::Restack {
                order: vec![ids[0], ids[0], ids[1], ids[2]],
                groups: vec![0; 4],
            })
            .is_err()
        );
    }

    #[test]
    fn align_and_distribute() {
        let mut e = editor();
        let a = add(&mut e, rect(10, 10, 20, 20));
        let b = add(&mut e, rect(50, 40, 10, 10));
        let c = add(&mut e, rect(200, 5, 30, 30));
        e.apply(Command::Align {
            ids: vec![a, b, c],
            edge: AlignEdge::Top,
        })
        .unwrap();
        assert!(
            [a, b, c]
                .iter()
                .all(|id| e.doc.get(*id).unwrap().rect.y == 5)
        );
        e.apply(Command::Distribute {
            ids: vec![a, b, c],
            axis: Axis::Horizontal,
        })
        .unwrap();
        // Span 10..230, sizes 20+10+30 = 60, gaps (220-60)/2 = 80: b starts at 30+80 = 110.
        assert_eq!(e.doc.get(b).unwrap().rect.x, 110);
        assert_eq!(e.doc.get(a).unwrap().rect.x, 10);
        assert_eq!(e.doc.get(c).unwrap().rect.x, 200);
        // One mark aligns to the frame.
        e.apply(Command::Align {
            ids: vec![b],
            edge: AlignEdge::Right,
        })
        .unwrap();
        assert_eq!(e.doc.get(b).unwrap().rect.right(), 400);
    }

    #[test]
    fn rotate_and_crop_are_undoable() {
        let mut e = editor();
        let id = add(&mut e, rect(10, 20, 30, 40));
        e.apply(Command::SetCrop {
            rect: Some(IRect::new(5, 5, 100, 100)),
        })
        .unwrap();
        e.apply(Command::Rotate { quarters: 1 }).unwrap();
        assert_eq!(e.doc.image_size(), (300, 400));
        assert_eq!(e.doc.get(id).unwrap().rot, 90);
        e.apply(Command::Undo).unwrap();
        assert_eq!(e.doc.image_size(), (400, 300));
        assert_eq!(e.doc.get(id).unwrap().rect, IRect::new(10, 20, 30, 40));
        // Full-picture crop is the same as no crop.
        e.apply(Command::SetCrop {
            rect: Some(IRect::new(-10, -10, 1000, 1000)),
        })
        .unwrap();
        assert_eq!(e.doc.crop, None);
        assert!(
            e.apply(Command::SetCrop {
                rect: Some(IRect::new(500, 500, 10, 10))
            })
            .is_err()
        );
    }

    #[test]
    fn resize_bakes_a_new_original_and_undo_returns_the_old() {
        let mut e = editor();
        let id = add(&mut e, rect(100, 100, 40, 40));
        e.apply(Command::Rotate { quarters: 1 }).unwrap();
        e.apply(Command::ResizeImage {
            width: 150,
            height: 200,
            scale_text: false,
        })
        .unwrap();
        assert_eq!(e.doc.image_size(), (150, 200));
        assert_eq!(e.doc.recipe.rot_quarters, 0);
        assert_eq!(e.doc.source, 1);
        e.apply(Command::Undo).unwrap();
        assert_eq!(e.doc.source, 0);
        assert_eq!(e.doc.recipe.rot_quarters, 1);
        assert_eq!(e.doc.image_size(), (300, 400));
        assert!(e.doc.get(id).is_some());
        assert!(
            e.apply(Command::ResizeImage {
                width: 0,
                height: 10,
                scale_text: false
            })
            .is_err()
        );
    }

    #[test]
    fn hostile_json_from_an_agent_cannot_break_the_core() {
        let mut e = editor();
        let big = r#"{"cmd":"add_object","object":{"rect":{"x":-2147483648,"y":2147483647,"w":2147483647,"h":-2147483648},"data":{"kind":"pen","points":[[-2147483648,2147483647],[2147483647,-2147483648]]}}}"#;
        let a = e.apply_json(big).unwrap().created.unwrap();
        let b = add(&mut e, rect(i32::MAX, i32::MIN, i32::MAX, i32::MAX));
        let c = add(&mut e, rect(0, 0, 10, 10));
        for cmd in [
            format!(r#"{{"cmd":"move_objects","ids":[{a},{b}],"dx":2147483647,"dy":-2147483648}}"#),
            format!(r#"{{"cmd":"resize_object","id":{b},"handle":4,"orig":{{"x":2147483647,"y":0,"w":2147483647,"h":1}},"dx":2147483647,"dy":2147483647}}"#),
            format!(r#"{{"cmd":"distribute","ids":[{a},{b},{c}],"axis":"horizontal"}}"#),
            format!(r#"{{"cmd":"align","ids":[{a},{b},{c}],"edge":"h_center"}}"#),
            r#"{"cmd":"rotate","quarters":-2147483648}"#.to_string(),
            r#"{"cmd":"set_crop","rect":{"x":-2147483648,"y":-2147483648,"w":2147483647,"h":2147483647}}"#.to_string(),
        ] {
            let _ = e.apply_json(&cmd);
        }
        for o in &e.doc.objects {
            let b = o.bounds();
            assert!(b.x.abs() <= COORD_LIMIT && b.w <= 2 * COORD_LIMIT, "{b:?}");
        }
        let _ = e.query(&Query::ListObjects);
        let _ = e.query(&Query::HitTest {
            x: f64::MAX,
            y: f64::MIN,
            px_per_doc: Some(0.0),
        });
    }

    #[test]
    fn tone_drag_is_one_step_and_clamped() {
        let mut e = editor();
        for v in [0.5f32, 1.0, 3.0] {
            e.apply(Command::SetTone {
                exposure: Some(v),
                gamma: None,
                contrast: None,
                merge: Some(MergeKey::Drag { id: 1 }),
            })
            .unwrap();
        }
        assert_eq!(e.doc.recipe.exposure, 2.0);
        e.apply(Command::Undo).unwrap();
        assert_eq!(e.doc.recipe.exposure, 0.0);
    }

    #[test]
    fn patch_rules() {
        let mut e = editor();
        let t = add(
            &mut e,
            Object::new(
                IRect::new(0, 0, 50, 20),
                Data::Text {
                    text: "a".into(),
                    size: 20,
                    bold: false,
                    italic: false,
                    align: Align::Left,
                    box_w: 0,
                },
            ),
        );
        let r = add(&mut e, rect(0, 0, 10, 10));
        let patch = ObjectPatch {
            text: Some("Привіт".into()),
            bold: Some(true),
            rot: Some(450),
            name: Some(Some("напис".into())),
            style: Some(StylePatch {
                alpha: Some(0),
                color2: Some(Some(Rgb::WHITE)),
                ..Default::default()
            }),
            ..Default::default()
        };
        e.apply(Command::UpdateObjects {
            ids: vec![t],
            patch,
            merge: None,
        })
        .unwrap();
        let o = e.doc.get(t).unwrap();
        assert!(matches!(&o.data, Data::Text { text, bold: true, .. } if text == "Привіт"));
        assert_eq!(o.rot, 90);
        assert_eq!(o.style.alpha, 10);
        assert_eq!(o.style.color2, Some(Rgb::WHITE));
        assert_eq!(o.name.as_deref(), Some("напис"));
        // Clearing the name and the second colour.
        let json = format!(
            r#"{{"cmd":"update_objects","ids":[{t}],"patch":{{"name":null,"style":{{"color2":null}}}}}}"#
        );
        e.apply_json(&json).unwrap();
        let o = e.doc.get(t).unwrap();
        assert_eq!(o.name, None);
        assert_eq!(o.style.color2, None);
        // Kind cannot change through data.
        let bad = ObjectPatch {
            data: Some(Data::Ellipse),
            ..Default::default()
        };
        assert!(
            e.apply(Command::UpdateObjects {
                ids: vec![r],
                patch: bad,
                merge: None
            })
            .is_err()
        );
        assert!(matches!(
            e.apply(Command::DeleteObjects { ids: vec![999] }),
            Err(CoreError::UnknownObject(999))
        ));
    }

    #[test]
    fn app_commands_are_not_in_core_and_versions_are_checked() {
        let mut e = editor();
        assert!(matches!(e.apply(Command::Save), Err(CoreError::NotInCore(n)) if n == "save"));
        assert!(e.apply_json(r#"{"v":1,"cmd":"select_all"}"#).is_ok());
        assert!(matches!(
            e.apply_json(r#"{"v":2,"cmd":"select_all"}"#),
            Err(CoreError::BadRequest(_))
        ));
        assert!(matches!(
            e.apply_json(r#"{"cmd":"set_name","name":"x","extra":1}"#),
            Err(CoreError::BadRequest(_))
        ));
        let id = add(&mut e, rect(10, 10, 20, 20));
        let hit = e
            .query_json(r#"{"query":"hit_test","x":15,"y":15}"#)
            .unwrap();
        assert_eq!(hit, format!(r#"{{"result":"hit","id":{id}}}"#));
    }

    #[test]
    fn timeline_and_marks_share_one_undo_history() {
        let mut e = editor();
        e.doc.timeline = Some(Timeline::whole(90));
        let first = add(&mut e, rect(10, 10, 20, 20));
        let mut cut = Timeline::whole(90);
        cut.parts = vec![
            TimelinePart {
                a: 0,
                b: 30,
                off: false,
            },
            TimelinePart {
                a: 30,
                b: 60,
                off: true,
            },
            TimelinePart {
                a: 60,
                b: 90,
                off: false,
            },
        ];
        e.apply(Command::SetTimeline {
            timeline: cut.clone(),
            merge: None,
        })
        .unwrap();
        // A drag of the out handle: many commands, one step.
        let k = MergeKey::Drag { id: 99 };
        for out in [85, 80, 75] {
            let mut t = cut.clone();
            t.out_point = out;
            e.apply(Command::SetTimeline {
                timeline: t,
                merge: Some(k.clone()),
            })
            .unwrap();
        }
        let second = add(&mut e, rect(50, 50, 20, 20));
        assert_eq!(e.doc.timeline.as_ref().unwrap().out_point, 75);
        e.apply(Command::Undo).unwrap();
        assert!(e.doc.get(second).is_none());
        e.apply(Command::Undo).unwrap();
        assert_eq!(e.doc.timeline.as_ref(), Some(&cut), "the drag is one step");
        e.apply(Command::Undo).unwrap();
        assert_eq!(e.doc.timeline, Some(Timeline::whole(90)));
        assert!(e.doc.get(first).is_some());
        e.apply(Command::Undo).unwrap();
        assert!(e.doc.get(first).is_none());
        for _ in 0..4 {
            e.apply(Command::Redo).unwrap();
        }
        assert!(e.doc.get(second).is_some());
        assert_eq!(e.doc.timeline.as_ref().unwrap().out_point, 75);
        // The number of frames cannot change; a gap is refused; a screenshot has no timeline.
        let mut bad = cut.clone();
        bad.parts[2].b = 100;
        assert!(
            e.apply(Command::SetTimeline {
                timeline: bad,
                merge: None
            })
            .is_err()
        );
        let mut gap = cut.clone();
        gap.parts[1].a = 31;
        assert!(
            e.apply(Command::SetTimeline {
                timeline: gap,
                merge: None
            })
            .is_err()
        );
        let mut shot = editor();
        assert!(
            shot.apply(Command::SetTimeline {
                timeline: cut,
                merge: None
            })
            .is_err()
        );
    }
}

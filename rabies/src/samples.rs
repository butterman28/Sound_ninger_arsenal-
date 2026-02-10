use parking_lot::RwLock;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct SampleMark {
    pub id: usize,             // ✅ NEW: Unique marker ID
    pub sample_path: String,
    pub sample_name: String,
    pub position: f32,         // Normalized 0.0-1.0
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct MarkerRelation {
    pub from_marker: usize,
    pub to_markers: Vec<usize>,  // Can end at any of these markers
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlaybackMode {
    PlayToEnd,
    PlayToNextMarker,
    CustomRegion { from: usize, to: usize },
}

pub struct SamplesManager {
    marks: RwLock<Vec<SampleMark>>,
    next_id: RwLock<usize>,
    relations: RwLock<HashMap<usize, Vec<usize>>>,  // from_marker -> [to_markers]
    pub playback_mode: RwLock<PlaybackMode>,
}

impl SamplesManager {
    pub fn new() -> Self {
        Self {
            marks: RwLock::new(Vec::new()),
            next_id: RwLock::new(1),  // Start from 1 for user-friendly numbering
            relations: RwLock::new(HashMap::new()),
            playback_mode: RwLock::new(PlaybackMode::PlayToEnd),
        }
    }

    pub fn mark_current_position(&self, sample_path: &str, sample_name: &str, position: f32) {
        let mut next_id = self.next_id.write();
        let id = *next_id;
        *next_id += 1;
        
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        
        let mark = SampleMark {
            id,
            sample_path: sample_path.to_string(),
            sample_name: sample_name.to_string(),
            position,
            timestamp,
        };
        
        self.marks.write().push(mark);
    }

    pub fn get_marks(&self) -> Vec<SampleMark> {
        self.marks.read().clone()
    }

    pub fn get_marks_for_sample(&self, sample_name: &str) -> Vec<SampleMark> {
        self.marks
            .read()
            .iter()
            .filter(|m| m.sample_name == sample_name)
            .cloned()
            .collect()
    }

    pub fn clear_marks(&self) {
        self.marks.write().clear();
        self.relations.write().clear();
    }

    pub fn update_mark_position(&self, index: usize, new_position: f32) {
        if let Some(mark) = self.marks.write().get_mut(index) {
            mark.position = new_position.clamp(0.0, 1.0);
        }
    }

    pub fn find_mark_near(&self, sample_name: &str, position: f32, threshold: f32) -> Option<usize> {
        let marks = self.marks.read();
        marks.iter().enumerate().find(|(_, mark)| {
            mark.sample_name == sample_name && (mark.position - position).abs() < threshold
        }).map(|(idx, _)| idx)
    }

    // ✅ NEW: Add custom relation between markers
    pub fn add_relation(&self, from_marker: usize, to_markers: Vec<usize>) {
        self.relations.write().insert(from_marker, to_markers);
    }

    // ✅ NEW: Get possible end markers for a given start marker
    pub fn get_end_markers_for(&self, from_marker: usize) -> Vec<usize> {
        self.relations
            .read()
            .get(&from_marker)
            .cloned()
            .unwrap_or_default()
    }

    // ✅ NEW: Get the target position based on current playback mode and position
    pub fn get_playback_target(&self, current_pos: f32, sample_name: &str) -> Option<f32> {
        let mode = self.playback_mode.read().clone();
        let marks = self.get_marks_for_sample(sample_name);
        
        // ✅ FIXED: Add minimum distance threshold to avoid immediate stops
        // 0.005 = 0.5% of file length
        // For a 60s file: 0.5% = 300ms minimum distance
        // For a 10s file: 0.5% = 50ms minimum distance
        const MIN_DISTANCE: f32 = 0.005;
        
        match mode {
            PlaybackMode::PlayToEnd => None,  // Play until the end
            PlaybackMode::PlayToNextMarker => {
                // Find the next marker that is meaningfully ahead
                marks
                    .iter()
                    .filter(|m| m.position > current_pos + MIN_DISTANCE)
                    .min_by(|a, b| a.position.partial_cmp(&b.position).unwrap())
                    .map(|m| m.position)
            }
            PlaybackMode::CustomRegion { from, to } => {
                // Find the "to" marker's position
                marks.iter().find(|m| m.id == to).map(|m| m.position)
            }
        }
    }

    // ✅ NEW: Check if we should stop playback at current position
    pub fn should_stop_at(&self, current_pos: f32, sample_name: &str) -> bool {
        if let Some(target) = self.get_playback_target(current_pos, sample_name) {
            current_pos >= target
        } else {
            false
        }
    }

    // ✅ NEW: Set playback mode
    pub fn set_playback_mode(&self, mode: PlaybackMode) {
        *self.playback_mode.write() = mode;
    }

    // ✅ NEW: Get current playback mode
    pub fn get_playback_mode(&self) -> PlaybackMode {
        self.playback_mode.read().clone()
    }

    // ✅ NEW: Delete a specific marker
    pub fn delete_mark(&self, index: usize) {
        let mut marks = self.marks.write();
        if index < marks.len() {
            let removed_id = marks.remove(index).id;
            
            // Also remove any relations involving this marker
            let mut relations = self.relations.write();
            relations.remove(&removed_id);
            for (_, to_markers) in relations.iter_mut() {
                to_markers.retain(|&id| id != removed_id);
            }
        }
    }

    // ✅ NEW: Get marker by ID
    pub fn get_mark_by_id(&self, id: usize) -> Option<SampleMark> {
        self.marks.read().iter().find(|m| m.id == id).cloned()
    }
}
//! Safe owner for a bounded Unicode buffer and the unchanged stb_textedit state.
use std::{
    alloc::{Layout, alloc_zeroed, dealloc},
    ffi::c_void,
    ops::Range,
    ptr::NonNull,
};
pub(super) const LIMIT: usize = 256 * 1024;
pub(super) const LEFT: i32 = 1;
pub(super) const RIGHT: i32 = 2;
pub(super) const UP: i32 = 3;
pub(super) const DOWN: i32 = 4;
pub(super) const PAGE_UP: i32 = 5;
pub(super) const PAGE_DOWN: i32 = 6;
pub(super) const HOME: i32 = 7;
pub(super) const END: i32 = 8;
pub(super) const START: i32 = 9;
pub(super) const FINISH: i32 = 10;
pub(super) const DELETE: i32 = 11;
pub(super) const BACKSPACE: i32 = 12;
pub(super) const UNDO: i32 = 13;
pub(super) const REDO: i32 = 14;
pub(super) const WORD_LEFT: i32 = 15;
pub(super) const WORD_RIGHT: i32 = 16;
unsafe extern "C" {
    fn repo_editor_size() -> usize;
    fn repo_editor_align() -> usize;
    fn repo_editor_init(state: *mut c_void);
    fn repo_editor_selection(state: *mut c_void, out: *mut i32);
    fn repo_editor_select_all(state: *mut c_void, length: i32);
    fn repo_editor_key(
        state: *mut c_void,
        text: *mut u32,
        length: *mut i32,
        capacity: i32,
        columns: i32,
        key: i32,
        shift: i32,
    );
    fn repo_editor_paste(
        state: *mut c_void,
        text: *mut u32,
        length: *mut i32,
        capacity: i32,
        input: *const u32,
        count: i32,
    ) -> i32;
    fn repo_editor_pointer(
        state: *mut c_void,
        text: *mut u32,
        length: i32,
        columns: i32,
        x: f32,
        y: f32,
        drag: i32,
    );
    fn repo_editor_row(text: *mut u32, length: i32, columns: i32, start: i32) -> i32;
}
pub(super) struct Buffer {
    state: NonNull<c_void>,
    allocation: Layout,
    data: Vec<u32>,
    length: i32,
    pub columns: usize,
}
// The C state contains no references or callbacks. Its single owner only calls C via &mut self.
unsafe impl Send for Buffer {}
unsafe impl Sync for Buffer {}
impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            dealloc(self.state.as_ptr().cast(), self.allocation);
        }
    }
}
impl Buffer {
    pub fn new(text: &str) -> Result<Self, String> {
        validate(text)?;
        let allocation =
            unsafe { Layout::from_size_align(repo_editor_size(), repo_editor_align()) }
                .map_err(|_| "Invalid editor state layout")?;
        let state = NonNull::new(unsafe { alloc_zeroed(allocation) }.cast::<c_void>())
            .ok_or("Could not allocate editor state")?;
        let mut data = vec![0; LIMIT];
        let mut length = 0;
        for ch in text.chars() {
            data[length] = ch as u32;
            length += 1;
        }
        unsafe {
            repo_editor_init(state.as_ptr());
        }
        Ok(Self {
            state,
            allocation,
            data,
            length: length as i32,
            columns: 80,
        })
    }
    pub fn chars(&self) -> &[u32] {
        &self.data[..self.length as usize]
    }
    pub fn text(&self) -> String {
        self.chars()
            .iter()
            .map(|&c| char::from_u32(c).expect("only Unicode scalar inputs enter stb"))
            .collect()
    }
    pub fn positions(&self) -> (usize, Range<usize>) {
        let mut out = [0; 3];
        // This function reads C state only; the adapter does not mutate through this pointer.
        unsafe {
            repo_editor_selection(self.state.as_ptr(), out.as_mut_ptr());
        }
        (
            out[0] as usize,
            out[1].min(out[2]) as usize..out[1].max(out[2]) as usize,
        )
    }
    pub fn selection(&self) -> String {
        self.chars()[self.positions().1]
            .iter()
            .filter_map(|&c| char::from_u32(c))
            .collect()
    }
    pub fn select_all(&mut self) {
        unsafe {
            repo_editor_select_all(self.state.as_ptr(), self.length);
        }
    }
    pub fn insert(&mut self, text: &str) -> Result<(), String> {
        validate(text)?;
        let selected = self.selection().len();
        if self.text().len() - selected + text.len() > LIMIT {
            return Err("Draft exceeds the 256 KiB editing limit.".into());
        }
        let input: Vec<u32> = text.chars().map(|ch| ch as u32).collect();
        let ok = unsafe {
            repo_editor_paste(
                self.state.as_ptr(),
                self.data.as_mut_ptr(),
                &mut self.length,
                LIMIT as i32,
                input.as_ptr(),
                input.len() as i32,
            )
        };
        if ok == 0 {
            return Err("Could not insert text; draft kept unchanged.".into());
        }
        Ok(())
    }
    pub fn key(&mut self, key: i32, shift: bool) {
        unsafe {
            repo_editor_key(
                self.state.as_ptr(),
                self.data.as_mut_ptr(),
                &mut self.length,
                LIMIT as i32,
                self.columns.max(1) as i32,
                key,
                shift as i32,
            );
        }
    }
    pub fn pointer(&mut self, x: f32, y: f32, drag: bool) {
        unsafe {
            repo_editor_pointer(
                self.state.as_ptr(),
                self.data.as_mut_ptr(),
                self.length,
                self.columns.max(1) as i32,
                x.max(0.),
                y.max(0.),
                drag as i32,
            );
        }
    }
    pub fn rows(&mut self) -> Vec<Range<usize>> {
        let mut rows = Vec::new();
        let mut start = 0;
        while start < self.length {
            let count = unsafe {
                repo_editor_row(
                    self.data.as_mut_ptr(),
                    self.length,
                    self.columns.max(1) as i32,
                    start,
                )
            };
            assert!(
                count > 0 && count <= self.length - start,
                "bounded layout must advance"
            );
            rows.push(start as usize..(start + count) as usize);
            start += count;
        }
        if self.length == 0 || self.data[self.length as usize - 1] == '\n' as u32 {
            rows.push(self.length as usize..self.length as usize);
        }
        rows
    }
}
pub(super) fn validate(text: &str) -> Result<(), String> {
    if text.len() > LIMIT {
        return Err("Draft exceeds the 256 KiB editing limit.".into());
    }
    if text.contains('\0') {
        return Err("NUL bytes cannot be edited as text.".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typing_unicode_selection_and_history_round_trip() {
        // Given a source draft, editing must preserve Unicode characters and undo state.
        let mut edit = Buffer::new("let α = 1;\n").unwrap();
        edit.select_all();
        edit.insert("∀ x, x ≤ 𝓟\n👀").unwrap();
        assert_eq!(edit.text(), "∀ x, x ≤ 𝓟\n👀");
        edit.key(UNDO, false);
        assert_eq!(edit.text(), "let α = 1;\n");
        edit.key(REDO, false);
        assert_eq!(edit.text(), "∀ x, x ≤ 𝓟\n👀");
        edit.key(LEFT, true);
        assert_eq!(edit.selection(), "👀");
    }
    #[test]
    fn rejected_paste_preserves_text_and_selection() {
        let mut edit = Buffer::new("source").unwrap();
        edit.select_all();
        assert!(edit.insert(&"x".repeat(LIMIT + 1)).is_err());
        assert_eq!(edit.text(), "source");
        assert_eq!(edit.selection(), "source");
    }
}

#[cfg(test)]
#[test]
fn wrapping_and_pointer_use_the_same_character_geometry() {
    let mut edit = Buffer::new("123456789\nαβγ").unwrap();
    edit.columns = 4;
    assert_eq!(edit.rows(), vec![0..4, 4..8, 8..10, 10..13]);
    edit.pointer(2., 1.2, false);
    assert_eq!(edit.positions().0, 6);
    edit.pointer(3., 1.2, true);
    assert_eq!(edit.selection(), "7");
    edit.key(UNDO, false);
    assert_eq!(
        edit.text(),
        "123456789\nαβγ",
        "navigation must not create undo edits"
    );
}

#[cfg(test)]
#[test]
fn moving_down_to_a_shorter_line_stops_before_its_newline() {
    let mut edit = Buffer::new("abcdef\nx\nnext").unwrap();
    edit.key(END, false);
    assert_eq!(edit.positions().0, 6);
    edit.key(DOWN, false);
    assert_eq!(edit.positions().0, 8);
    edit.key(UP, false);
    assert_eq!(
        edit.positions().0,
        6,
        "vertical navigation keeps its preferred column"
    );
}

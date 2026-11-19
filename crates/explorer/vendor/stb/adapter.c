/* Repo City adapter. The vendored stb header is unchanged. No platform API or libc. */
#ifdef _MSC_VER
#include <stddef.h>
#include <intrin.h>
#define EDITOR_ALIGNOF(t) __alignof(t)
#define EDITOR_TRAP() __debugbreak()
#else
typedef __SIZE_TYPE__ size_t;
#define EDITOR_ALIGNOF(t) _Alignof(t)
#define EDITOR_TRAP() __builtin_trap()
#endif
#define NULL ((void *)0)
static void *editor_move(void *to, const void *from, size_t n) {
    unsigned char *d = to; const unsigned char *s = from;
    if (d < s) { for (size_t i = 0; i < n; ++i) d[i] = s[i]; }
    else { while (n) { --n; d[n] = s[n]; } }
    return to;
}
#define STB_TEXTEDIT_CHARTYPE unsigned int
#define STB_TEXTEDIT_UNDOSTATECOUNT 256
#define STB_TEXTEDIT_UNDOCHARCOUNT 262144
#define STB_TEXTEDIT_POSITIONTYPE int
#include "stb_textedit.h"
typedef struct { unsigned int *text; int length, capacity, columns; } EditorString;
static int editor_insert(EditorString *s, int at, const unsigned int *text, int n) {
    if (at < 0 || at > s->length || n < 0 || n > s->capacity - s->length) return 0;
    editor_move(s->text + at + n, s->text + at, (size_t)(s->length - at) * sizeof(*text));
    for (int i = 0; i < n; ++i) s->text[at + i] = text[i];
    s->length += n; return 1;
}
static void editor_delete(EditorString *s, int at, int n) {
    if (at < 0 || n < 0 || at > s->length || n > s->length - at) EDITOR_TRAP();
    editor_move(s->text + at, s->text + at + n, (size_t)(s->length - at - n) * sizeof(*s->text));
    s->length -= n;
}
static float editor_width(unsigned int c) { return c == '\t' ? 4.f : c == '\n' || c == '\r' ? 0.f : 1.f; }
static void editor_row(StbTexteditRow *row, EditorString *s, int start) {
    int n = 0; float width = 0;
    while (start + n < s->length) {
        unsigned int c = s->text[start + n];
        float next = editor_width(c);
        if (n && width + next > s->columns) break;
        ++n; width += next;
        if (c == '\n') break;
    }
    row->x0 = 0; row->x1 = width; row->ymin = 0; row->ymax = 1;
    row->baseline_y_delta = 1; row->num_chars = n;
}
#define STB_TEXTEDIT_STRING EditorString
#define STB_TEXTEDIT_STRINGLEN(s) ((s)->length)
#define STB_TEXTEDIT_LAYOUTROW(row,s,start) editor_row(row,s,start)
#define STB_TEXTEDIT_GETWIDTH_NEWLINE (-1.f)
#define STB_TEXTEDIT_GETWIDTH(s,start,i) ((s)->text[(start)+(i)] == '\n' ? STB_TEXTEDIT_GETWIDTH_NEWLINE : editor_width((s)->text[(start)+(i)]))
#define STB_TEXTEDIT_GETCHAR(s,i) ((s)->text[i])
#define STB_TEXTEDIT_NEWLINE '\n'
#define STB_TEXTEDIT_DELETECHARS(s,i,n) editor_delete(s,i,n)
#define STB_TEXTEDIT_INSERTCHARS(s,i,p,n) editor_insert(s,i,p,n)
#define STB_TEXTEDIT_IS_SPACE(c) ((c) == ' ' || (c) == '\t' || (c) == '\n' || (c) == '\r')
#define STB_TEXTEDIT_KEYTOTEXT(k) (-1)
#define STB_TEXTEDIT_K_SHIFT 0x10000
#define STB_TEXTEDIT_K_LEFT 1
#define STB_TEXTEDIT_K_RIGHT 2
#define STB_TEXTEDIT_K_UP 3
#define STB_TEXTEDIT_K_DOWN 4
#define STB_TEXTEDIT_K_PGUP 5
#define STB_TEXTEDIT_K_PGDOWN 6
#define STB_TEXTEDIT_K_LINESTART 7
#define STB_TEXTEDIT_K_LINEEND 8
#define STB_TEXTEDIT_K_TEXTSTART 9
#define STB_TEXTEDIT_K_TEXTEND 10
#define STB_TEXTEDIT_K_DELETE 11
#define STB_TEXTEDIT_K_BACKSPACE 12
#define STB_TEXTEDIT_K_UNDO 13
#define STB_TEXTEDIT_K_REDO 14
#define STB_TEXTEDIT_K_WORDLEFT 15
#define STB_TEXTEDIT_K_WORDRIGHT 16
#define STB_TEXTEDIT_memmove editor_move
#define STB_TEXTEDIT_IMPLEMENTATION
#include "stb_textedit.h"
size_t repo_editor_size(void) { return sizeof(STB_TexteditState); }
size_t repo_editor_align(void) { return EDITOR_ALIGNOF(STB_TexteditState); }
void repo_editor_init(void *state) { stb_textedit_initialize_state(state, 0); }
void repo_editor_selection(void *state, int *out) {
    STB_TexteditState *s = state; out[0] = s->cursor; out[1] = s->select_start; out[2] = s->select_end;
}
void repo_editor_select_all(void *state, int length) {
    STB_TexteditState *s = state; s->cursor = length; s->select_start = 0; s->select_end = length;
}
void repo_editor_key(void *state, unsigned int *text, int *length, int capacity, int columns, int key, int shift) {
    EditorString s = {text, *length, capacity, columns};
    ((STB_TexteditState *)state)->row_count_per_page = 20;
    stb_textedit_key(&s, state, key | (shift ? STB_TEXTEDIT_K_SHIFT : 0)); *length = s.length;
}
int repo_editor_paste(void *state, unsigned int *text, int *length, int capacity, const unsigned int *input, int count) {
    EditorString s = {text, *length, capacity, 80};
    STB_TexteditState *e = state;
    int begin = e->select_start < e->select_end ? e->select_start : e->select_end;
    int end = e->select_start > e->select_end ? e->select_start : e->select_end;
    if (end > begin) {
        if (count > capacity - (s.length - (end - begin))) return 0;
        /* One user replacement is one undo step. Keep upstream header unchanged. */
        stb_text_makeundo_replace(&s, e, begin, end - begin, count);
        editor_delete(&s, begin, end - begin);
        editor_insert(&s, begin, input, count);
        e->cursor = begin + count; e->has_preferred_x = 0;
    } else if (!stb_textedit_paste(&s, state, input, count)) return 0;
    e->select_start = e->select_end = e->cursor;
    *length = s.length; return 1;
}
void repo_editor_pointer(void *state, unsigned int *text, int length, int columns, float x, float y, int drag) {
    EditorString s = {text, length, length, columns};
    if (drag) stb_textedit_drag(&s, state, x, y); else stb_textedit_click(&s, state, x, y);
}
int repo_editor_row(unsigned int *text, int length, int columns, int start) {
    EditorString s = {text, length, length, columns}; StbTexteditRow row;
    editor_row(&row, &s, start); return row.num_chars;
}

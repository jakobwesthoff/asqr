# Options with an empty description are not shown

Status: seen on 2026-10-03 in daily use (asqr 0.9.1), cause found and
confirmed by a test; not fixed yet. The user asked for this todo.

## What the user saw

A single question with three options listed only the third one and the
own-answer line. The first two options were missing, with no gap and no
renumbering: the list started at "3.". The cursor pointer was not
visible either. The preselected option was one of the hidden ones, so
the cursor sat on a row that drew nothing. The user could not pick the
hidden options and typed the answer into the own-answer line instead.
A second question with two options, both with empty descriptions,
showed no options at all.

Screenshot, with the project's texts and images blanked out:
`01m40nrm5qrebqb93fztcpythc-empty-description-hides-option.png`.

## Reproduction

`01m40nrm5qrebqb93fztcpythc-empty-description-hides-option.json` is a
minimal session (it validates):

- `empty`: two options with `"description": ""` and one with text. Only
  the one with text shows.
- `allempty`: both options with `"description": ""`. Neither shows.
- `control`: the same options with the description left out entirely.
  All of them show.

Drop it with `asqr ask` and open it. The affected sessions also had
images, but the cause does not depend on them.

## Cause

In `src/tui/render/screen.rs`, `option_lines` returns the head alone
only when the description is `None`. For `Some("")`,
`markdown::render("")` yields no lines, so both branches loop over
nothing. The function returns an empty `Vec`, and the option row has no
lines at all: label, mark and number vanish with the description.

Confirmed with a temporary test in the `screen.rs` test module, which
was reverted afterwards. It is the regression test to add:

    #[test]
    fn an_option_with_an_empty_description_still_shows() {
        let json = r#"{"asqr": 1, "questions": [{"id": "q", "text": "Which one?", "kind": "single",
            "options": [
                {"id": "a", "label": "Alpha option", "description": ""},
                {"id": "b", "label": "Beta option", "description": ""},
                {"id": "c", "label": "Gamma option", "description": "Has a description."}]}]}"#;
        let app = app(&[("probe", json)]);
        let text = format!("{:?}", screen(&app, 120, 20));
        assert!(text.contains("Gamma option"), "control option shows");
        assert!(text.contains("Alpha option"), "option with empty description shows");
    }

It failed on the second assertion: the control option showed and the
option with the empty description did not.

## Fix direction (not decided)

- In `option_lines`, fall back to the head-only line whenever the
  rendered description has no lines, not only for `None`. This also
  covers descriptions that render to nothing, such as whitespace only.
- Possibly treat an empty or whitespace-only `description` as absent
  when the session is read, and let `validate` warn about it, since an
  agent writing `""` most likely meant "no description".
- Check the other callers of `markdown::render` on optional texts for
  the same pattern.

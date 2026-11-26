# Keys

The full table is
[`docs/keymap.md`](https://github.com/lampseen/pullpen/blob/main/docs/keymap.md),
generated from the code, and `?` inside pullpen shows the same table.

The ones to know first:

| key | does |
|---|---|
| `j` `k` `gg` `G` `^d` `^u` | move |
| `h` `l` `Tab` `1`–`9` | sections |
| `Enter` | open the PR |
| `/` then `n` `N` | filter, next and previous match |
| `Space` `V` | mark a row, mark a range |
| `M` `A` `a` `c` `x` | merge, arm, approve, comment, close |
| `u` | update the branch from its base |
| `D` | in Deploys, dismiss a failing branch |
| `w` | the PR's pipeline, live: see [Watching pipelines](Watching-pipelines) |
| `o` `d` | browser, diff |
| `:` `?` `q` | command line, help, back |

In the detail view, `h` and `l` move between the overview and the diff,
`n` and `b` move to the next and previous file, `F` lists every file to
jump to, `]c` `[c` move between hunks, `zo` `zc` unfold and
fold the checks, and `d` widens the diff.

Single keys can be rebound by name in `config.toml`, under `[keys]`:
`merge = "m"` adds `m` for merging, and `M` keeps working.

## Mouse

Optional: everything below is also a key. Each line is a test in
`crates/pullpen-ui/src/app/mouse/tests.rs`.

| mouse | does | test |
|---|---|---|
| click a tab | that section | `clicking_a_tab_switches_and_a_row_selects_and_twice_opens` |
| click a row | selects it: a PR, a job, a step, a file in `F` | `a_click_in_a_grouped_view_selects_the_row_under_it_not_a_heading`, `a_job_row_selects_and_twice_opens_the_job`, `a_picker_row_selects_and_twice_opens_the_file` |
| double-click a row | `Enter` on it | `a_double_click_in_deploys_opens_the_pipeline` |
| click a pane | `h` or `l` | `clicking_a_pane_focuses_it` |
| click the top row of a PR, pipeline or job | `q` | `the_breadcrumb_goes_back_one_level` |
| wheel | scrolls what is under the pointer | `the_wheel_scrolls_the_pane_under_it_and_the_log_stops_following`, `the_wheel_stops_at_the_end_of_a_pane` |

A confirm prompt, the command line, a filter and `?` take keys only
(`a_click_at_a_confirm_prompt_changes_nothing`).

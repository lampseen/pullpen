# Commands

## On the command line

| | |
|---|---|
| `pullpen` | open the queue |
| `pullpen --fixtures` | the design's sample data, offline |
| `pullpen --section <name>` | open on a section |
| `pullpen setup` | sign in and answer what pullpen needs |
| `pullpen setup <host>` | sign in to one forge again and ask nothing else |
| `pullpen settings` | the settings menu |
| `pullpen settings get <key>` | one setting's value |
| `pullpen settings set <key> <value>` | change one setting |
| `pullpen settings path` | where the config and credentials files are |
| `pullpen config check` | what works, and the fix for what does not |
| `pullpen --version` | the version |

## Inside pullpen

`:` opens a command line. `Tab` completes commands, PR numbers and section
names; `↑` `↓` walk history, filtered by what you have typed.

| | |
|---|---|
| `:merge [#n] [squash\|merge\|rebase]` | merge, after confirming |
| `:automerge [#n] [method\|off]` | arm, re-arm with another method, or disarm. `:am` for short |
| `:approve [#n]` | approve |
| `:comment [#n] <text>` | comment; bare `:comment` opens `$EDITOR` |
| `:checkout [#n]` | check out the head branch in your clone |
| `:open [#n]` | open in the browser |
| `:diff [#n]` | open the diff |
| `:close [#n]` | close without merging, after confirming |
| `:update [#n]` | bring the branch up to date with its base: GitHub merges the base in, GitLab rebases |
| `:refresh [all]` | fetch this section, or all |
| `:section <name>` | switch section |
| `:messages` | the last 200 results and errors |
| `:mouse` | turn the mouse off, and again to turn it back on, until you quit |
| `:clear` | in the Merged section, drop every finished row that is not failing |
| `:dismiss [#n]` | in Deploys, resolve a failing branch by hand |
| `:watch [#n]` | the PR's pipeline and its jobs' logs, live |
| `:rerun [#n]` | run the failed jobs of the PR's newest pipeline again |
| `:cancel [#n]` | stop the jobs of that pipeline that are still running |
| `:ready [#n]` `:draft [#n]` | mark ready for review, or put back to draft |
| `:reopen [#n]` | reopen a closed PR |
| `:request-changes [#n] <text>` | review asking for changes. On GitLab the text is posted as a comment |
| `:review [#n] @login…` | ask for a review, beside whoever was already asked |
| `:label [#n] +name -name…` | add and remove labels; the answer lists the labels the PR has afterwards |
| `:assign [#n] [@login…]` | assign people, beside whoever already is; bare `:assign` is yourself |
| `:title [#n] <text>` | change the title |
| `:new [base]` | open a PR for the branch checked out where pullpen was started, into `base` or the repo's default branch. The title and description are that branch's newest commit; it asks first. The branch must already be pushed |
| `:q` `:qa` | back, or quit |

Without a number, a command acts on the selected row, or on every marked row.
`#812` and `!812` both work; a bare number is the row as displayed.

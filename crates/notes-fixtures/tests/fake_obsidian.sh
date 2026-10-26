#!/bin/sh
# A stand-in for the Obsidian CLI, written out as `obsidian` by the tests. It logs every call and
# answers in the shapes https://obsidian.md/help/cli shows: names or paths one per line, a count for
# `total`, JSON for `format=json`. Anything that page leaves out (the JSON fields) is made up.
#   FAKE_OBSIDIAN_LOG         file each call is appended to (required)
#   FAKE_OBSIDIAN_VERSION     what `version` prints, even when empty (default: 1.14.5 (installer 1.12.7))
#   FAKE_OBSIDIAN_FAIL        a command name that prints an error and exits 1
#   FAKE_OBSIDIAN_FILES_HIDE  a path `files` leaves out
#   FAKE_OBSIDIAN_FILES_EXTRA a path `files` adds
#   FAKE_OBSIDIAN_ERROR       a command name that prints `Error: ...` and exits 0, as the real app does
#   FAKE_OBSIDIAN_HANG        a command name that never answers
#   FAKE_OBSIDIAN_HANG_ONCE   a command name that never answers the first time it is called
#   FAKE_OBSIDIAN_VAULT_PATH  the path `vault` reports instead of the working directory
#   FAKE_OBSIDIAN_DIAGNOSTIC  text printed to stderr on every call
#   FAKE_OBSIDIAN_OPEN        answer to the openFile eval (default: => preview)
#   FAKE_OBSIDIAN_DOM_MISSING number of dev:dom not-found answers after each open
#   FAKE_OBSIDIAN_DOM_CHANGES number of dev:dom answers after those that differ each time, as a view still rendering
#   FAKE_OBSIDIAN_DOM         final dev:dom answer, including an explicitly empty answer
log="${FAKE_OBSIDIAN_LOG:?FAKE_OBSIDIAN_LOG is required}"
printf '%s\n' "$HOME" >> "$log.home"
{
  printf 'cwd\037%s\n' "$PWD"
  printf 'argv'
  for a in "$@"; do printf '\037%s' "$a"; done
  printf '\n'
} >> "$log"

if [ -n "${FAKE_OBSIDIAN_DIAGNOSTIC:-}" ]; then
  printf '%s\n' "$FAKE_OBSIDIAN_DIAGNOSTIC" >&2
fi
case "$1" in vault=*) shift ;; esac
cmd="$1"
shift
if [ "$cmd" = "${FAKE_OBSIDIAN_FAIL:-}" ]; then
  echo "Error: $cmd failed" >&2
  exit 1
fi
if [ "$cmd" = "${FAKE_OBSIDIAN_ERROR:-}" ]; then
  echo "Error: $cmd could not be run"
  exit 0
fi
if [ "$cmd" = "${FAKE_OBSIDIAN_HANG:-}" ]; then
  exec sleep 30
fi
if [ "$cmd" = "${FAKE_OBSIDIAN_HANG_ONCE:-}" ] && [ ! -e "$log.hung" ]; then
  : > "$log.hung"
  exec sleep 30
fi

total=""
format=""
view=""
selector=""
for a in "$@"; do
  case "$a" in
    total) total=1 ;;
    format=*) format="${a#format=}" ;;
    view=*) view="${a#view=}" ;;
    selector=*) selector="${a#selector=}" ;;
  esac
done

case "$cmd" in
  version) echo "${FAKE_OBSIDIAN_VERSION-1.14.5 (installer 1.12.7)}" ;;
  vault)
    shown="${FAKE_OBSIDIAN_VAULT_PATH:-$PWD}"
    case " $* " in
      *" info=path "*) echo "$shown" ;;
      *" info=name "*) basename "$shown" ;;
      *) printf 'name\t%s\npath\t%s\n' "$(basename "$shown")" "$shown" ;;
    esac
    ;;
  help) echo "fake help: commands are listed one per line" ;;
  files)
    list=$(find . -type f -not -path '*/.*' | sed 's|^\./||' | sort)
    if [ -n "${FAKE_OBSIDIAN_FILES_HIDE:-}" ]; then
      list=$(printf '%s\n' "$list" | grep -vxF "$FAKE_OBSIDIAN_FILES_HIDE")
    fi
    if [ -n "${FAKE_OBSIDIAN_FILES_EXTRA:-}" ]; then
      list=$(printf '%s\n%s\n' "$list" "$FAKE_OBSIDIAN_FILES_EXTRA")
    fi
    if [ -n "$total" ]; then printf '%s\n' "$list" | wc -l | tr -d ' '; else printf '%s\n' "$list"; fi
    ;;
  folders) find . -type d -not -path '*/.*' -not -name . | sed 's|^\./||' | sort ;;
  bases) find . -name '*.base' -not -path '*/.*' | sed 's|^\./||' | sort ;;
  plugins:enabled | bookmarks) echo '[]' ;;
  templates | snippets:enabled) ;;
  unresolved)
    if [ -n "$total" ]; then echo 3; else echo '[{"link":"Missing","count":2}]'; fi
    ;;
  orphans | deadends)
    if [ -n "$total" ]; then echo 1; else echo "Orphan.md"; fi
    ;;
  links) echo "Dead End.md" ;;
  backlinks) echo '[{"file":"Home.md","count":1}]' ;;
  outline) echo '[{"heading":"Home","level":1}]' ;;
  aliases) echo "Start" ;;
  properties) echo '[{"name":"tags","count":1}]' ;;
  tags) echo '[{"tag":"#home","count":1}]' ;;
  tasks) echo '[{"path":"tasks/Task Statuses.md","line":8,"status":" ","text":"To do"}]' ;;
  search)
    if [ -n "$total" ]; then echo 1; elif [ "$format" = json ]; then echo '["Home.md"]'; else echo "Home.md"; fi
    ;;
  search:context)
    if [ "$format" = json ]; then echo '[{"path":"Home.md","line":1,"text":"# Home"}]'; else echo "Home.md:1: # Home"; fi
    ;;
  base:query) printf '[{"path":"projects/Project Alpha.md","view":"%s"}]\n' "$view" ;;
  wordcount) printf 'words\t12\ncharacters\t80\n' ;;
  eval)
    case "$1" in
      code=*openFile*)
        printf '%s' "$1" | sed -n 's/.*getFileByPath(\("[^"]*"\)).*/\1/p' > "$log.active"
        printf '0' > "$log.dom-attempts"
        printf '%s\n' "${FAKE_OBSIDIAN_OPEN-=> preview}"
        ;;
      code=*unresolvedLinks*) echo '=> {"Home.md":{"Missing":1}}' ;;
      code=*resolvedLinks*) echo '=> {"Home.md":{"Dead End.md":1}}' ;;
      code=*getCache*) printf '%s\n' '"{\"Home.md\":{\"links\":[]}}"' ;;
      *) echo '=> 7' ;;
    esac
    ;;
  dev:dom)
    attempt=$(cat "$log.dom-attempts")
    attempt=$((attempt + 1))
    printf '%s' "$attempt" > "$log.dom-attempts"
    if [ "$attempt" -le "${FAKE_OBSIDIAN_DOM_MISSING:-0}" ]; then
      echo 'Error: Command "dev:dom" not found. It may require a plugin to be enabled.'
      exit 0
    fi
    if [ "$attempt" -le $(( ${FAKE_OBSIDIAN_DOM_MISSING:-0} + ${FAKE_OBSIDIAN_DOM_CHANGES:-0} )) ]; then
      printf '<div class="markdown-reading-view">rendering, read %s</div>\n' "$attempt"
      exit 0
    fi
    if [ "${FAKE_OBSIDIAN_DOM+x}" ]; then
      printf '%s\n' "$FAKE_OBSIDIAN_DOM"
      exit 0
    fi
    printf '<div class="markdown-reading-view" data-selector="%s">fake dom for %s</div>\n' "$selector" "$(cat "$log.active" 2>/dev/null)"
    ;;
  *)
    echo "Error: unknown command $cmd" >&2
    exit 1
    ;;
esac

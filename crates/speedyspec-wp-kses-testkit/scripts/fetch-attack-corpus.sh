#!/usr/bin/env bash
# Fetches the upstream XSS and expansion-attack sources into target/attack-sources/ (never committed), at their
# current default-branch commit, sparse where a repository is large. Prints each source's commit; the extract
# script then copies the payloads into corpus/attacks/ with their licenses.
set -euo pipefail
root="$(cd "$(dirname "$0")/../../.." && pwd)"
dest="$root/target/attack-sources"
mkdir -p "$dest"
fetch() {
	local repo="$1" subdir="${2:-}" name
	name="$(basename "$repo")"
	if [ ! -d "$dest/$name" ]; then
		if [ -n "$subdir" ]; then
			git clone -q --depth 1 --filter=blob:none --sparse "https://github.com/$repo.git" "$dest/$name"
			git -C "$dest/$name" sparse-checkout set --no-cone "/$subdir/" "/LICENSE*" "/README*" >/dev/null
		else
			git clone -q --depth 1 "https://github.com/$repo.git" "$dest/$name"
		fi
	fi
	echo "$repo $(git -C "$dest/$name" rev-parse HEAD)"
}
fetch cure53/H5SC
fetch cure53/DOMPurify test/fixtures
fetch swisskyrepo/PayloadsAllTheThings "XSS Injection"
fetch danielmiessler/SecLists Fuzzing/XSS
fetch OWASP/CheatSheetSeries cheatsheets

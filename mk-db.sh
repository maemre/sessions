#!/usr/bin/env zsh

setopt -euo pipefail

sqlite3 bookmarks.db <schema.sql
sqlite3 bookmarks.db <fixtures.sql

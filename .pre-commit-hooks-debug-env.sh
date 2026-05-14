#!/usr/bin/env bash
env | sort | grep -E '^(PRE_COMMIT|SKIP)' || true
exit 0

#!/bin/bash

set -e -o pipefail

BENCH_DIR=".bench-results"

usage() {
    echo "Usage:"
    echo "  $0 run <label>         Run benchmarks and save results with a label"
    echo "  $0 compare <a> <b>     Compare two labeled benchmark runs"
    echo "  $0 list                List saved benchmark results"
    echo "  $0 latest              Show the most recent result"
    echo ""
    echo "Examples:"
    echo "  $0 run before          # baseline run"
    echo "  # ... make changes ..."
    echo "  $0 run after           # run after changes"
    echo "  $0 compare before after"
}

cmd_run() {
    local label="$1"
    if [ -z "$label" ]; then
        echo "Error: label is required"
        usage
        exit 1
    fi

    mkdir -p "$BENCH_DIR"

    local outfile="$BENCH_DIR/$label.txt"
    echo "Running benchmarks (label: $label)..."
    echo ""

    cargo bench -p oxanus 2>&1 | tee "$outfile"

    echo ""
    echo "Results saved to $outfile"
}

cmd_compare() {
    local a="$1"
    local b="$2"

    if [ -z "$a" ] || [ -z "$b" ]; then
        echo "Error: two labels are required"
        usage
        exit 1
    fi

    local file_a="$BENCH_DIR/$a.txt"
    local file_b="$BENCH_DIR/$b.txt"

    if [ ! -f "$file_a" ]; then
        echo "Error: no results found for '$a' ($file_a)"
        exit 1
    fi
    if [ ! -f "$file_b" ]; then
        echo "Error: no results found for '$b' ($file_b)"
        exit 1
    fi

    # Extract timing lines: bench name and time
    extract_timings() {
        grep -E '^\s+│' "$1" | sed 's/│/|/g' || true
    }

    echo "=== Benchmark Comparison: $a vs $b ==="
    echo ""

    # Show them side by side using diff
    paste <(
        echo "--- $a ---"
        grep -E '(╰─|│|├─)' "$file_a" || grep -E '^\s' "$file_a"
    ) <(
        echo "--- $b ---"
        grep -E '(╰─|│|├─)' "$file_b" || grep -E '^\s' "$file_b"
    ) | column -t -s $'\t' 2>/dev/null || {
        # Fallback: show sequentially
        echo "--- $a ---"
        cat "$file_a"
        echo ""
        echo "--- $b ---"
        cat "$file_b"
    }

    echo ""
    echo "Full results:"
    echo "  $file_a"
    echo "  $file_b"
}

cmd_list() {
    if [ ! -d "$BENCH_DIR" ]; then
        echo "No benchmark results found. Run '$0 run <label>' first."
        exit 0
    fi

    echo "Saved benchmark results:"
    for f in "$BENCH_DIR"/*.txt; do
        [ -f "$f" ] || continue
        local label=$(basename "$f" .txt)
        local date=$(stat -f "%Sm" -t "%Y-%m-%d %H:%M" "$f" 2>/dev/null || stat -c "%y" "$f" 2>/dev/null | cut -d. -f1)
        printf "  %-20s %s\n" "$label" "$date"
    done
}

cmd_latest() {
    if [ ! -d "$BENCH_DIR" ]; then
        echo "No benchmark results found."
        exit 1
    fi

    local latest=$(ls -t "$BENCH_DIR"/*.txt 2>/dev/null | head -1)
    if [ -z "$latest" ]; then
        echo "No benchmark results found."
        exit 1
    fi

    echo "Latest: $(basename "$latest" .txt)"
    echo ""
    cat "$latest"
}

# Main
case "${1:-}" in
    run)     cmd_run "$2" ;;
    compare) cmd_compare "$2" "$3" ;;
    list)    cmd_list ;;
    latest)  cmd_latest ;;
    *)       usage ;;
esac

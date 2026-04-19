#!/usr/bin/env sh

# Run an agent with the configured runner (claude or codex).
#   run_agent <log-file> <prompt>
run_agent() {
    output_file=$1
    prompt=$2

    if [ "${RUNNER:-codex}" = "claude" ]; then
        claude --dangerously-skip-permissions \
            --model "${CLAUDE_MODEL:-opus}" \
            --verbose \
            --output-format text \
            --max-turns 500 \
            -p "$prompt" 2>&1 | tee "$output_file"
    else
        codex exec \
            --enable multi_agent \
            -m "${CODEX_MODEL:-gpt-5.4}" \
            -s danger-full-access \
            "$prompt" 2>&1 | tee "$output_file"
    fi
}

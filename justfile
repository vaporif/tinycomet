build:
    cargo build

start: build
    cargo run -p tinycomet

run-app:
    cargo run -p tinycomet-app -- --socket /tmp/app.sock --db-path ./data/tinycomet.db

run-proxy:
    cargo run -p tinycomet-proxy -- --app-socket /tmp/app.sock --cmt-socket /tmp/cmt.sock

install-cometbft:
    GOBIN=/usr/local/bin/ go install github.com/cometbft/cometbft/cmd/cometbft@v0.38.21

testnet-up:
    docker compose up --build

testnet-down:
    docker compose down -v

prune:
    rm -rf ~/.cometbft
    rm -rf ./data/tinycomet.db
    rm -f /tmp/app.sock /tmp/cmt.sock

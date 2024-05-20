# Run some pre-commit checks.
precommit:
    cargo clippy
    nix run nixpkgs#typos

# Redeploy a new version of the server.
refresh-server:
    docker compose up --detach --build server

# Redeploy all services, rebuilding `server` and `sqlrunner`.
redeploy:
    docker compose down
    docker compose up --detach --build server postgresql pgadmin
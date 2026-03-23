# Usage

Full command reference for the nyne CLI. For VFS filesystem workflows (reading, writing, refactoring via `@/` paths), see **[Workflows](WORKFLOWS.md)**.

## `nyne mount`

Start FUSE daemon(s) for one or more directories. Defaults to the current directory.

```sh
nyne mount                              # mount cwd
nyne mount /path/to/project             # mount a specific directory
nyne mount myid:/path/to/project        # mount with explicit session ID
nyne mount /path/a /path/b              # mount multiple directories
```

## `nyne attach`

Spawn a process inside a running mount's sandboxed namespace. Defaults to `$SHELL`.

```sh
nyne attach                             # attach to the only active mount
nyne attach myid                        # attach to a specific session
nyne attach -- claude                   # run a command in the namespace
```

### `--visibility`

Controls whether `@/` companion directories appear in directory listings.

| Value | Behavior |
|-|-|
| `default` | Companion dirs exist but are hidden from `ls`. Access by name works. |
| `all` | Companion dirs appear in directory listings. |
| `none` | Full passthrough — process sees only the real filesystem. |

```sh
nyne attach --visibility all            # agents see everything in ls
nyne attach --visibility none           # disable VFS entirely
```

## `nyne list`

Show active sessions, or processes attached to a specific session.

```sh
nyne list                               # all active sessions
nyne list myid                          # processes attached to a session
```

## `nyne exec`

Pipe-oriented script execution against a running daemon. Binary stdin/stdout.

```sh
nyne exec provider.claude.post-tool-use
nyne exec --id myid provider.claude.post-tool-use
```

## `nyne ctl`

Send a JSON control request to a running daemon. Reads from an argument or stdin.

```sh
nyne ctl '{"type": "status"}'
echo '{"type": "status"}' | nyne ctl
nyne ctl --id myid '{"type": "status"}'
```

## `nyne config`

Dump the resolved configuration with all defaults applied.

```sh
nyne config
```

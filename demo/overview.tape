Source demo/setup.tape

Output demo/overview.mp4

Type "cd /code/plugins/coding/src/syntax"
Enter
Show
Sleep 1s

Type "glow -w0 fragment.rs@/OVERVIEW.md"
Sleep 500ms
Enter
Sleep 5s

Type "bat fragment.rs@/symbols/"
Tab@500ms 2
Sleep 200ms
Type "Fragment~Impl@/"
Tab@500ms 2
Sleep 200ms
Type "new@/body.rs"
Sleep 500ms
Enter
Sleep 3s

Type "glow -w0 fragment.rs@/symbols/Fragment~Impl@/new@/CALLERS.md"
Sleep 500ms
Enter
Sleep 3s

Type "eza -1 fragment.rs@/symbols/Fragment~Impl@/new@/callers/"
Sleep 500ms
Enter
Sleep 3s

Type "bat fragment.rs@/symbols/Fragment~Impl@/new@/callers/build_code_frag"
Tab
Type "/body.rs"
Sleep 500ms
Enter
Sleep 3s

Type "eza -1 fragment.rs@/symbols/Fragment~Impl@/new@/git/history/"
Sleep 500ms
Enter
Sleep 3s

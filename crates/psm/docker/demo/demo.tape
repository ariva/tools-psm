# VHS script for docs/assets/demo.gif: what psm shows around a synthetic workload.
# Played by the psm-demo image (`just demo`); output lands in the working directory, /out, the volume `just demo` mounts.
Output demo.gif

Set Shell bash
Set FontSize 15
Set Width 1280
Set Height 760
Set Padding 16
Set Framerate 20
Set TypingSpeed 40ms
Set Theme "Catppuccin Mocha"

# `enter`: a private PID namespace with fake /proc/meminfo and /proc/loadavg, then a `dev` shell in it.
Hide
Type "exec /opt/demo/enter"
Enter
Type "export PS1='\[\e[1;32m\]dev@demo-box\[\e[0m\]:\[\e[1;34m\]~\[\e[0m\]$ ' && cd ~ && psm init >/dev/null 2>&1; clear"
Enter
Show

Type "psm new release-v1"
Enter
Sleep 2.5s

Type "./workload start"
Enter
Sleep 1.5s

Type "psm procs"
Enter
Sleep 3s

Type "psm snap after-start"
Enter
Sleep 2s

Type "psm diff prev --watch 1"
Enter
Sleep 5s
Ctrl+C
Sleep 1s

Type "psm track -- ./workload build"
Enter
Sleep 6.5s

Type "psm report trend"
Enter
Sleep 3s

Type "psm new release-v2 && ./workload restart 96"
Enter
Sleep 3.5s

Type "psm snap after-start"
Enter
Sleep 2.5s

Type "psm sessions compare release-v1 release-v2"
Enter
Sleep 3.5s

Type "psm diff 0 1 --json | jq -c '.data.new[]'"
Enter
Sleep 4s

# svc
A simple service & util manager for Windows.

## Configuration
```yaml
# services.yaml
# do not use other file names

- name: MyServer
  type: Executable
  path: D:\path\to\server.exe
  work_at: D:\dir

- name: MyTool
  type: Executable
  path: D:\path\to\tool.exe
  # default working dir:
  # work_at: D:\path\to\

# item with the type `util` will be invoked by custom interpreter
- name: js
  type: Util
  path: D:\path\to\my\script.js
  interpreter: nodejs # default interpreter is "python"
  # work_at: ...
```

## Usage
```shell
# add start-up task for Executable
svc enable MyServer

# svc will not automatically run your program
# so do not ignore this:
svc run MyServer

# kill by:
# (be careful that this command will kill all processes
# that have the same executable path as "MyServer")
svc kill MyServer

# disable by:
svc disable MyServer

# quick run your programs or scripts
svc run MyTool
svc run js

# custom working dir at run-time
# this will overwrite `work_at` property in config
svc run MyTool at "D:\"

# check status
svc status MyServer
```
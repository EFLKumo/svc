# svc
A simple service & util manager for Windows.

## Configuration
```yaml
# services.yaml
# do not use other file names

- name: MyServer
  type: Executable
  path: D:\path\to\server.exe

- name: MyTool
  type: Executable
  path: D:\path\to\tool.exe

# item with the type `util` will be invoked by python.exe
- name: PyScript
  type: Util
  path: D:\path\to\python\script.py
```

## Usage
```shell
# add start-up task for Executable
svc enable MyServer

# svc will not automatically run your program
# so do not ignore this:
svc run MyServer

# kill by:
svc kill MyServer

# disable by:
svc disable MyServer

# quick run your programs or scripts
svc run MyTool
svc run PyScript

# check status
svc status MyServer
```
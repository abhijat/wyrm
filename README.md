# Wyrm

Dragonfly MCP server

## Requirements

- pip or npm for installing
- Linux x86-64 and glibc 2.28+

## Installation

### Pip

`pip install dragonfly-wyrm`

and to update

`pip install --upgrade dragonfly-wyrm`

### NPM

`npm install -g dragonfly-wyrm`

## Run

After any of the above, the `wyrm` executable can be run.

## Command Autogeneration

Dragonfly's RESP commands are generated at from [json schema](commands.json). Each entry in that file is
converted to a tool call registered with the server. The tool call results in RESP command sent to connected server, and
the response is converted from redis type to json value before returning.

Note that the string itself is compiled into the binary, so the json file does not need to be distributed with the
binary.
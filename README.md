[![GLWTPL](https://img.shields.io/badge/GLWT-Public_License-red.svg)](https://spdx.org/licenses/GLWTPL.html)

Ferricorn
---------
![](images/ferricorn_logo2.png)

Ferricorn is a 100% Rust ASGI server.
It's a experimental project born from a conversation I had during FOSDEM 2025.


## Running

```shell
$ cargo run -- "echo_server:app"
```

This command will run the echo server included in the repository.


## Architecture

The core idea is to have a Python interpreter per thread. The HTTP connections are all handled with Tokio Async and communicated with the thread that runs Python through a `mpsc` channel.

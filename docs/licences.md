# Licences

[← README](../README.md)

## Augmented View

The plugin and the server are released under the MIT licence ([LICENSE](../LICENSE)).

## Third-party code

The third-party licences of the crates the plugin and the server are built from, and of the native code compiled into
the server, are in `licences/`: `plugin.txt` and `server.txt`. Each server release carries `server.txt` as
`augmented-view-server-THIRD-PARTY-LICENSES.txt`, with the plugin's licence as `augmented-view-server-LICENSE.txt`;
the release tool refuses to release the server without them. After a dependency changes, regenerate them with
`python licences/generate.py` (it needs Python 3.8+ and cargo-about).

`server/data/wordfreq-en.txt` is derived from wordfreq's English word list (Robyn Speer,
<https://github.com/rspeer/wordfreq>) and is under CC BY-SA 4.0.

## The models

The models are not distributed with the plugin or the server; you download them yourself
([Installation](installation.md#3-the-models)), and each keeps its own licence:

| Model | Licence |
|---|---|
| [BAAI/bge-base-en-v1.5](https://huggingface.co/BAAI/bge-base-en-v1.5) | [MIT](https://opensource.org/license/mit) |
| [Qwen/Qwen3-4B-Instruct-2507](https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507) | [Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0) |
| [fastino/gliner2-base-v1](https://huggingface.co/fastino/gliner2-base-v1) | [Apache-2.0](https://www.apache.org/licenses/LICENSE-2.0) |
| [UDPipe 2.5 English EWT](https://hdl.handle.net/11234/1-3131) | [CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/): **personal and non-commercial use only** |

A model you set by hand (*Model*) has the licence on its own page. The Models section of the settings, the server's
terminal question and its `missing` line all show each model's licence and page.

---
title: "Publish your first document"
description: "Start Keryx locally and turn an HTML file into a shareable document."
---

This guide assumes [Keryx is installed](../installation/). The default server listens on your own machine; sharing with other people needs a server address they can reach.

## 1. Start the server

```sh
keryx serve
```

Leave it running. By default, the server listens at `http://127.0.0.1:7812` and stores data under `~/.keryx`.

## 2. Write a document

Save this as `plan.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Release plan</title>
    <style>
      body { max-width: 48rem; margin: 3rem auto; padding: 0 1rem; font: 1rem/1.6 system-ui; }
      h1 { line-height: 1.1; }
    </style>
  </head>
  <body>
    <header><h1>Release plan</h1><p>A small first release.</p></header>
    <main><section><h2>Next steps</h2><p>Finish the docs and share the build.</p></section></main>
  </body>
</html>
```

## 3. Upload it

In another terminal, run this from the directory containing the file:

```sh
keryx upload ./plan.html
```

The CLI prints the public URL, raw HTML URL, draft ID and version number. Open the public URL in a browser. Hand the raw URL to another agent when it needs to read the document.

## 4. Revise it

Edit the file and upload the same path again:

```sh
keryx upload ./plan.html
```

The draft gets another version. Its main link serves the latest version; earlier versions keep their own links. Use `--draft <draft-id>` to explicitly target a draft from another path or session.

## 5. Share beyond your machine

A localhost link only works on the machine running the server. Configure a reachable server and its public base URL before sharing with others. See [server configuration](../configuration/).

Continue with [agent setup](../agents/) or [versions and links](../versions/).

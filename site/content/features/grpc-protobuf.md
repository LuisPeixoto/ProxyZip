---
title: gRPC and Protobuf debugging
description: Decode gRPC, gRPC-Web and Protobuf messages without a schema. Inspect fields, import descriptors for names and copy requests as code.
h1: Read gRPC and Protobuf without the schema
image: grpc
image_alt: A gRPC-Web response decoded into numbered protobuf fields
---

Binary protocols are hard to debug because you cannot read them. ProxyZip decodes Protobuf bodies on the wire and shows them as a tree, even when you do not have the `.proto` files.

## What is decoded

- **gRPC** and **gRPC-Web** messages, with the method, direction and each message in the stream
- Plain **Protobuf** request and response bodies
- Trailers such as `grpc-status`

Without a schema, fields are shown by number: `"1": "sku_1042"`, `"3": 4900`. ProxyZip infers the wire types and says so in the viewer, so you know the names are missing and not the data.

## Add names

Import a descriptor set to see real field names. Messages keep their numbers and gain labels, and the tree is searchable like any other body.

## Mock a gRPC service

A [mock route](../mock-api-server/) can return a file. Binary files are served byte for byte, so a captured gRPC-Web frame replays unchanged and your client can be tested against it.

## Work with the rest of the toolbox

- Filter the list with `type:protobuf` or by host
- Compare two messages with the **Diff** tab
- Copy the request as cURL or generate client code in 14 languages

See the [inspecting guide](../../docs/inspecting/#protobuf-and-grpc).

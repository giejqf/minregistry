# 9. Unread request bodies and connection reuse

Date: 2026-10-04 · Status: accepted

## Context

Many responses are decided before the request body is read: the 401 that
asks for credentials, a 403, a 416 for an out-of-order chunk, a 400 for an
invalid digest. Registry clients send their first request without
credentials and retry it with Basic auth after the 401, on the same
keep-alive connection. That includes the OCI conformance suite's client,
and with it every `PUT` and `PATCH` that carries a body.

When a handler drops an unread body, hyper (1.x) reads only what has
already arrived. If the body is still in flight, hyper closes the connection
after the response, but the response does not carry `Connection: close`
(the headers may already be written). The client puts the connection back
in its pool and sends the retry on it. That fails with EOF, and Go's HTTP
client does not replay a `PUT` on its own. CI saw this as an intermittent
conformance failure: `Put ".../blobs/uploads/<uuid>?digest=…": EOF` right
after a 401.

## Decision

A middleware, `unread_body::finish_request_body`, wraps every HTTP/1.x
request body and notes whether the handler read it to the end. If not:

- it reads and discards up to 256 KiB more, for at most 5 s, before
  returning the response, so the connection stays reusable (the same limit
  as Go's `net/http` server);
- if the body is larger, does not arrive in time, is still held elsewhere,
  or the client sent `Expect: 100-continue`, it adds `Connection: close` to
  the response instead, so the client opens a new connection. With
  `Expect: 100-continue` the client is waiting for `100 Continue` before it
  sends the body, and reading the body would make hyper send it.

HTTP/2 is left alone: it ends unread streams itself and forbids the
`Connection` header.

## Consequences

Early answers to small requests are delayed until their body has arrived.
That is usually a round trip at most, and never more than 5 s. An
unauthenticated client can make the server read at most 256 KiB per request,
which costs no more than a normal request does. Large uploads that are
rejected early close their connection instead of streaming megabytes for
nothing.

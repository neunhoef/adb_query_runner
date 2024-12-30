# ArangoDB query runner

This is an experiment to implement - with the help of some AI LLM - a
quick and dirty program to run some preconfigured queries against an
ArangoDB database. The result is then shown as JSON on the web front end.
If the result is a graph, it is also sent to a local Cytoscape instance
to be displayed.

This whole thing was generated with the help of the AI "Claude" by
Anthropic in half a day. It is not ready and basically not tested and
thus not ready for production use. Some error handling is missing and it
is so far not sufficiently protected against misuse.

Treat this as a proof of concept.

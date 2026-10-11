# provenance: original
"""The klein-science SymPy stand-in.

A guest program imports this package as `sympy`. Every computation leaves the guest as one tool call through
`sympy._tool.call`, encoded by the one Python JCS encoder, `sympy._wire`. Plan 01-12 generates the full name table
into this file.
"""

# Invalid gate run

The raw probe reported zero resize events, but the window never repainted after
its initial frame. The PNGs show the initial Loading card at every zoom, and
DPR remains 2. This is not a passing A1 gate or an R1 baseline. The continuation
adds window activation and validates the rendered camera before accepting a
sample. Raw JSON and PNGs are retained for diagnosis.

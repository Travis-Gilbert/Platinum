# Measured command runtimes

Exact arguments, environments and outputs are in [commands.json](commands.json). Read-only exploration and editing calls were not individually timed.

| Command | Exit | Accepted | Wall seconds | Log |
| --- | ---: | --- | ---: | --- |
| disk | 0 |  | 0.003946 | [disk-1793121285529976000.log](disk-1793121285529976000.log) |
| canvas-build | 0 |  | 16.989335 | [canvas-build-1793121285534491000.log](canvas-build-1793121285534491000.log) |
| disk | 0 |  | 0.004793 | [disk-1793121328849998000.log](disk-1793121328849998000.log) |
| frame-tests | 0 |  | 23.981168 | [frame-tests-1793121328855118000.log](frame-tests-1793121328855118000.log) |
| disk | 0 |  | 0.004694 | [disk-1793121385152720000.log](disk-1793121385152720000.log) |
| zoom | 0 | True | 7.797161 | [zoom-1793121385163856000.log](zoom-1793121385163856000.log) |
| disk | 0 |  | 0.005762 | [disk-1793121423714217000.log](disk-1793121423714217000.log) |
| signin | 0 | True | 81.217115 | [signin-1793121423728756000.log](signin-1793121423728756000.log) |
| disk | 0 |  | 0.005005 | [disk-1793121524440240000.log](disk-1793121524440240000.log) |
| measure | 0 | False | 128.151413 | [measure-1793121524450707000.log](measure-1793121524450707000.log) |

Final static checks and cleanup timings are in [verification.json](verification.json). The first formatting check recursed into production modules; the scoped new-file check passed.

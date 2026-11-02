# Failed native attempt

The probe aborted during native event dispatch: its typed WindowHandle held
Root mutably while dispatch needed Root again. No A1/A2/A3 result was produced.
The continuation uses AnyWindowHandle, which does not borrow the root entity.
See signin-1793117652515496000.log one directory up.

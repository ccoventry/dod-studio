// Run by vegas_instances.launch as `vegas210.exe -SCRIPT:start_vegas.cs`.
// It does nothing on purpose: a VEGAS started with a script opens an empty
// untitled project instead of its last one, which is often the user's own.
using ScriptPortal.Vegas;

public class EntryPoint {
    public void FromVegas(Vegas vegas) { }
}

// Compiles a VEGAS C# script the way VEGAS's own script compiler does, so
// run_script can refuse one that would fail there: VEGAS shows a compile
// error as a modal box. VEGAS compiles C# 5 (the .NET Framework csc) against
// a fixed set of assemblies, without System.Core (no LINQ, no HashSet;
// measured on build 315). CodeDom runs that same csc with /noconfig, so
// nothing outside this list is referenced. A script whose .config adds
// references, or a VEGAS build that references more, is refused here
// although VEGAS would run it.
using System;
using System.CodeDom.Compiler;
using System.Collections.Generic;
using System.IO;
using Microsoft.CSharp;

namespace DodStudio.VegasBridge {

public static class ScriptCheck {
    static readonly string[] References = {
        "System.dll", "System.Drawing.dll", "System.Windows.Forms.dll", "System.Data.dll", "System.Xml.dll",
    };

    // The compiler's errors as "line N: message", or none.
    public static List<string> Compile(string path) {
        CompilerParameters p = new CompilerParameters();
        foreach (string r in References) p.ReferencedAssemblies.Add(r);
        p.ReferencedAssemblies.Add(typeof(ScriptPortal.Vegas.Vegas).Assembly.Location);
        p.GenerateExecutable = false;
        p.GenerateInMemory = false;
        p.OutputAssembly = Path.Combine(Path.GetTempPath(), "dodstudio_vegas_check_" + Session.Pid + ".dll");
        List<string> errors = new List<string>();
        using (CSharpCodeProvider csc = new CSharpCodeProvider()) {
            CompilerResults res = csc.CompileAssemblyFromFile(p, path);
            foreach (CompilerError e in res.Errors)
                if (!e.IsWarning) errors.Add("line " + e.Line + ": " + e.ErrorNumber + " " + e.ErrorText);
        }
        try { File.Delete(p.OutputAssembly); } catch { }
        return errors;
    }
}

}

// Event groups (events that move together, such as a clip's video and
// audio): group_events, and the groups project_info shows. An event is in
// at most one group.
using System;
using System.Collections.Generic;
using ScriptPortal.Vegas;

namespace DodStudio.VegasBridge {

public partial class Commands {

    // The event's group as its position in the project's group list, the
    // number project_info's `groups` uses.
    int GroupIndex(TrackEvent ev) {
        int i = 0;
        foreach (TrackEventGroup g in P.TrackEventGroups) {
            if (g.Equals(ev.Group)) return i;
            i++;
        }
        return -1;
    }

    List<object> GroupsInfo() {
        List<object> r = new List<object>();
        foreach (TrackEventGroup g in P.TrackEventGroups) {
            List<object> members = new List<object>();
            foreach (TrackEvent ev in g) members.Add(EventRef(ev));
            r.Add(members);
        }
        return r;
    }

    static object EventRef(TrackEvent ev) {
        Dictionary<string, object> d = new Dictionary<string, object>();
        d["track"] = ev.Track.Index;
        d["event"] = ev.Index;
        return d;
    }

    // action "group": the events become one group (each leaves any group it
    // was in); "ungroup": each leaves its group. A group left with fewer
    // than two events goes.
    object GroupEvents(Args a) {
        string action = a.Str("action");
        if (action != "group" && action != "ungroup") throw new BridgeError("action is \"group\" or \"ungroup\"");
        List<TrackEvent> events = new List<TrackEvent>();
        foreach (Args e in a.Items("events")) events.Add(EventAt(e.Int("track"), e.Int("event")));
        if (action == "group" && events.Count < 2) throw new BridgeError("a group needs at least two events");
        foreach (TrackEvent ev in events) Ungroup(ev);
        if (action == "group") {
            TrackEventGroup g = new TrackEventGroup(P);
            P.TrackEventGroups.Add(g);
            foreach (TrackEvent ev in events) g.Add(ev);
        }
        Dictionary<string, object> r = new Dictionary<string, object>();
        r["groups"] = GroupsInfo();
        return r;
    }

    void Ungroup(TrackEvent ev) {
        if (!ev.IsGrouped) return;
        TrackEventGroup g = ev.Group;
        g.Remove(ev);
        if (g.Count >= 2) return;
        List<TrackEvent> rest = new List<TrackEvent>();
        foreach (TrackEvent other in g) rest.Add(other);
        foreach (TrackEvent other in rest) g.Remove(other);
        P.TrackEventGroups.Remove(g);
    }
}

}

import json, sys
def rec(hook):
    return {"call": "add_filter", "args": [hook, {"callback": "return_arg", "label": "rec:" + hook}, 10, 99]}
def answer(hook, fn):
    return {"call": "add_filter", "args": [hook, fn, 10, 99]}
def throw(hook):
    return {"call": "add_filter", "args": [hook, {"callback": "throw", "label": "throw:" + hook, "with": {"message": "boom"}}, 10, 99]}
PLAIN = {"add_filter", "update_option"}
def c(call, *args):
    if call in PLAIN:
        return {"call": call, "args": list(args)}
    return {"call": "speedyspec_cron_call", "args": [call] + list(args)}
WATCH = ["pre_schedule_event", "schedule_event", "pre_reschedule_event", "pre_unschedule_event",
         "pre_clear_scheduled_hook", "pre_unschedule_hook", "pre_get_scheduled_event", "wp_next_scheduled",
         "get_schedule", "pre_get_ready_cron_jobs", "pre_update_option_cron", "update_option_cron"]
def watched(*steps):
    return [rec(h) for h in WATCH] + list(steps)
S = []
def scenario(name, *steps):
    S.append({"name": name, "steps": list(steps)})
E = []
scenario("single_event", *watched(c("speedyspec_cron_single", 3600, "h"), c("speedyspec_cron_next", "h"), c("speedyspec_cron_array")))
scenario("single_event_args", *watched(c("speedyspec_cron_single", 3600, "h", ["a", 1, {"k": "v"}]),
    c("speedyspec_cron_next", "h", ["a", 1, {"k": "v"}]), c("speedyspec_cron_next", "h", ["a", 2])))
scenario("recurring_event", *watched(c("speedyspec_cron_event", 3600, "hourly", "h", [7]), c("speedyspec_cron_scheduled", "h", [7]), c("wp_get_schedule", "h", [7])))
for ts in [0, -1, "soon", "", None, True]:
    for wp_error in [False, True]:
        scenario(f"invalid_timestamp_{json.dumps(ts)}_{wp_error}".replace('"', ''), *watched(
            c("wp_schedule_single_event", ts, "h", [], wp_error), c("wp_schedule_event", ts, "hourly", "h", [], wp_error),
            c("wp_reschedule_event", ts, "hourly", "h", [], wp_error), c("wp_unschedule_event", ts, "h", [], wp_error)))
for wp_error in [False, True]:
    scenario(f"invalid_schedule_{wp_error}", *watched(c("speedyspec_cron_event", 60, "nope", "h", [], wp_error),
        c("speedyspec_cron_reschedule", 60, "nope", "h", [], wp_error)))
    scenario(f"duplicates_{wp_error}", *watched(c("speedyspec_cron_single", -100, "h", [], wp_error),
        c("speedyspec_cron_single", 590, "h", [], wp_error), c("speedyspec_cron_single", 3600, "h", [], wp_error),
        c("speedyspec_cron_single", 4199, "h", [], wp_error), c("speedyspec_cron_single", 4201, "h", [], wp_error),
        c("speedyspec_cron_single", 2399, "h", [], wp_error), c("speedyspec_cron_array")))
    for hook, call in [("pre_schedule_event", c("speedyspec_cron_single", 60, "h", [], wp_error)),
                       ("pre_schedule_event", c("speedyspec_cron_event", 60, "daily", "h", [], wp_error)),
                       ("pre_reschedule_event", c("speedyspec_cron_reschedule", 60, "daily", "h", [], wp_error)),
                       ("pre_unschedule_event", c("speedyspec_cron_unschedule", 60, "h", [], wp_error)),
                       ("pre_clear_scheduled_hook", c("wp_clear_scheduled_hook", "h", [], wp_error)),
                       ("pre_unschedule_hook", c("wp_unschedule_hook", "h", wp_error))]:
        for fn in ["__return_false", "__return_true", "speedyspec_cron_plugin_error", "speedyspec_cron_plugin_value", "__return_zero"]:
            inner = call['args'][0] if call['call'] == 'speedyspec_cron_call' else call['call']
            scenario(f"{hook}_{inner}_{fn}_{wp_error}", *watched(answer(hook, fn), call, c("speedyspec_cron_array")))
    scenario(f"schedule_event_filter_false_{wp_error}", *watched(answer("schedule_event", "__return_false"),
        c("speedyspec_cron_single", 60, "h", [], wp_error), c("speedyspec_cron_event", 60, "hourly", "h", [], wp_error)))
    scenario(f"could_not_set_{wp_error}", *watched(c("speedyspec_cron_unschedule", 60, "missing", [], wp_error),
        c("wp_unschedule_hook", "missing", wp_error), c("_set_cron_array", [], wp_error)))
    scenario(f"cron_array_error_via_pre_update_{wp_error}", *watched(answer("pre_update_option_cron", {"callback": "return_arg", "label": "keep_old", "with": {"index": 1}}),
        c("speedyspec_cron_single", 60, "h", [], wp_error), c("speedyspec_cron_event", 60, "hourly", "h", [], wp_error),
        c("wp_unschedule_hook", "h", wp_error)))
scenario("schedule_event_filter_moves", *watched(answer("schedule_event", "speedyspec_cron_shift_event"),
    c("speedyspec_cron_single", 3600, "moved"), c("speedyspec_cron_event", 7200, "hourly", "moved_too"), c("speedyspec_cron_array")))
scenario("unschedule_and_clear", *watched(c("speedyspec_cron_single", 100, "a"), c("speedyspec_cron_single", 2000, "a"),
    c("speedyspec_cron_single", 3000, "a", ["x"]), c("speedyspec_cron_event", 50, "daily", "b"),
    c("speedyspec_cron_unschedule_next", "a"), c("wp_clear_scheduled_hook", "a"), c("wp_clear_scheduled_hook", "a", ["x"]),
    c("wp_unschedule_hook", "b"), c("wp_unschedule_hook", "b"), c("speedyspec_cron_array")))
scenario("clear_merges_errors", *watched(c("speedyspec_cron_single", 100, "a"), c("speedyspec_cron_single", 4000, "a"),
    answer("pre_unschedule_event", "__return_false"), c("wp_clear_scheduled_hook", "a", [], True), c("wp_clear_scheduled_hook", "a", [], False)))
scenario("clear_discrete_arguments", *watched(c("speedyspec_cron_single", 100, "a", ["x", "y"]), c("wp_clear_scheduled_hook", "a", "x", "y"), c("speedyspec_cron_array")))
scenario("reschedule", *watched(c("speedyspec_cron_reschedule", -100, "hourly", "r"), c("speedyspec_cron_reschedule", 100, "daily", "r2"),
    c("speedyspec_cron_event", 100, "hourly", "r3"), c("speedyspec_cron_reschedule", 100, "gone", "r3", [], True), c("speedyspec_cron_array")))
scenario("schedules", rec("cron_schedules"), c("wp_get_schedules"), answer("cron_schedules", "speedyspec_cron_schedules"), c("wp_get_schedules"),
    c("speedyspec_cron_event", 60, "minute", "m"), c("speedyspec_cron_scheduled", "m"))
scenario("scheduled_event_lookups", *watched(c("speedyspec_cron_single", 100, "s"), c("speedyspec_cron_single", 5000, "s"),
    c("speedyspec_cron_scheduled", "s"), c("speedyspec_cron_scheduled", "s", [], 5000), c("speedyspec_cron_scheduled", "s", [], 300),
    c("wp_get_scheduled_event", "s", [], "soon"), c("speedyspec_cron_scheduled", "none"), c("wp_get_schedule", "s"), c("wp_get_schedule", "none")))
scenario("pre_get_scheduled_event_answers", *watched(answer("pre_get_scheduled_event", "speedyspec_cron_plugin_value"),
    c("wp_get_scheduled_event", "s"), c("wp_next_scheduled", "s"), c("wp_get_schedule", "s")))
scenario("ready_jobs", *watched(c("speedyspec_cron_single", -300, "past"), c("speedyspec_cron_single", -20, "past2"),
    c("speedyspec_cron_single", 3600, "future"), c("speedyspec_cron_ready")))
scenario("ready_jobs_filtered", *watched(answer("pre_get_ready_cron_jobs", "__return_empty_array"), c("speedyspec_cron_single", -300, "past"), c("speedyspec_cron_ready")))
scenario("legacy_array_is_upgraded", *watched(c("speedyspec_cron_legacy", 600, "old", ["a"]), c("speedyspec_cron_array"), c("speedyspec_cron_next", "old", ["a"])))
scenario("legacy_array_without_args", *watched(c("speedyspec_cron_raw", {"600": {"old": {"schedule": False}}}, None), c("speedyspec_cron_array")))
scenario("junk_in_the_option", *watched(c("speedyspec_cron_raw", {"600": {"a": {"40cd750bba9870f18aada2478b24840a": {"schedule": False, "args": []}}, "not_events": "x"}}, 2, {"junk": "kept"}),
    c("speedyspec_cron_single", 4000, "b"), c("speedyspec_cron_array"), c("speedyspec_cron_next", "a")))
scenario("non_array_option", *watched(c("update_option", "cron", "nonsense"), c("speedyspec_cron_array"), c("speedyspec_cron_single", 600, "a"), c("speedyspec_cron_array")))
scenario("filter_throws", throw("pre_schedule_event"), c("speedyspec_cron_single", 60, "h"), c("speedyspec_cron_array"))
scenario("numeric_hook_and_string_timestamp", *watched(c("speedyspec_cron_single", 600, "123"), c("speedyspec_cron_next", "123"), c("speedyspec_cron_array")))
scenario("set_cron_array_directly", *watched(c("_set_cron_array", "not an array"), c("speedyspec_cron_array"), c("_set_cron_array", [], True)))
names = [s["name"] for s in S]
assert len(names) == len(set(names)), "duplicate scenario names"
json.dump({"area": "cron", "scenarios": S}, sys.stdout, indent=2)
print()
print(len(S), file=sys.stderr)

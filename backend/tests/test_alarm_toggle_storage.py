from app.database.interaction import (
    get_alarms_enabled,
    load_toggle_history,
    set_alarms_enabled,
)


async def test_defaults_to_enabled_on_empty_table(db_session):
    assert await get_alarms_enabled(db_session) is True


async def test_disabling_records_a_transition(db_session):
    assert await set_alarms_enabled(db_session, False) is True
    assert await get_alarms_enabled(db_session) is False

    history = await load_toggle_history(db_session)
    assert len(history) == 1
    assert history[0].enabled is False
    assert history[0].changed_at is not None


async def test_restating_current_value_writes_nothing(db_session):
    # Table is empty, which already means enabled.
    assert await set_alarms_enabled(db_session, True) is False
    assert await load_toggle_history(db_session) == []

    await set_alarms_enabled(db_session, False)
    assert await set_alarms_enabled(db_session, False) is False
    assert len(await load_toggle_history(db_session)) == 1


async def test_history_is_newest_first_and_respects_limit(db_session):
    for enabled in (False, True, False):
        await set_alarms_enabled(db_session, enabled)

    history = await load_toggle_history(db_session)
    assert [e.enabled for e in history] == [False, True, False]

    assert len(await load_toggle_history(db_session, limit=2)) == 2

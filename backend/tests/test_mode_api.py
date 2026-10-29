async def test_get_alarms_defaults_to_enabled(client):
    response = await client.get("/api/mode/alarms")
    assert response.status_code == 200
    assert response.json() == {"enabled": True}


async def test_put_alarms_disables_and_persists(client):
    response = await client.put("/api/mode/alarms", json={"enabled": False})
    assert response.status_code == 200

    assert (await client.get("/api/mode/alarms")).json() == {"enabled": False}


async def test_put_alarms_succeeds_when_device_unreachable(client):
    # No fake Pico is running, so the AlarmsUpdated nudge fails. The write must stand:
    # backend ownership means the device catches up on its next poll.
    response = await client.put("/api/mode/alarms", json={"enabled": False})
    assert response.status_code == 200
    assert (await client.get("/api/mode/alarms")).json() == {"enabled": False}


async def test_history_records_transitions_newest_first(client):
    await client.put("/api/mode/alarms", json={"enabled": False})
    await client.put("/api/mode/alarms", json={"enabled": True})

    history = (await client.get("/api/mode/alarms/history")).json()
    assert [entry["enabled"] for entry in history] == [True, False]


async def test_history_ignores_repeated_values(client):
    await client.put("/api/mode/alarms", json={"enabled": False})
    await client.put("/api/mode/alarms", json={"enabled": False})

    assert len((await client.get("/api/mode/alarms/history")).json()) == 1


async def test_animation_rejects_off(client):
    response = await client.post("/api/mode/animation", json={"type": "off"})
    assert response.status_code == 422


async def test_animation_forwards_to_device(client, pico):
    await pico(b"OK: Command queued\n")

    response = await client.post("/api/mode/animation", json={"type": "rainbow"})
    assert response.status_code == 200

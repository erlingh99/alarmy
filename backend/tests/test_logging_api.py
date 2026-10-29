class TestPostLog:
    async def test_post_valid_log(self, client):
        response = await client.post("/api/logs", json=["hello from device"])
        assert response.status_code == 200
        assert response.json()["message"] == "Logs saved successfully"

    async def test_post_empty_array(self, client):
        response = await client.post("/api/logs", json=[])
        assert response.status_code == 422

    async def test_post_oversized_message(self, client):
        response = await client.post("/api/logs", json=["x" * (256 + 1)])
        assert response.status_code == 422

    async def test_post_batch_log(self, client):
        response = await client.post(
            "/api/logs", json=["first message", "second message", "third message"]
        )
        assert response.status_code == 200

        response = await client.get("/api/logs")
        assert len(response.json()) == 3


class TestGetLogs:
    async def test_get_logs_empty(self, client):
        response = await client.get("/api/logs")
        assert response.status_code == 200
        assert response.json() == []

    async def test_get_logs_after_post(self, client):
        await client.post("/api/logs", json=["test message"])
        response = await client.get("/api/logs")
        assert response.status_code == 200
        assert len(response.json()) == 1
        assert response.json()[0]["message"] == "test message"

    async def test_get_logs_max_param(self, client):
        await client.post("/api/logs", json=[f"msg {i}" for i in range(5)])
        response = await client.get("/api/logs?max=3")
        assert response.status_code == 200
        assert len(response.json()) == 3

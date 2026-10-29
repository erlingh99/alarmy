from dataclasses import dataclass


@dataclass
class Model:
    name: str


def test_model_creation():
    # Example test case for model creation
    model = Model(name="Test Model")
    assert model.name == "Test Model"

"""add alarm play_melody

Revision ID: ad919ca85ae3
Revises: 2c76ad58d077
Create Date: 2026-09-17 00:00:00.000000

"""

from collections.abc import Sequence

import sqlalchemy as sa
from alembic import op

# revision identifiers, used by Alembic.
revision: str = "ad919ca85ae3"
down_revision: str | Sequence[str] | None = "2c76ad58d077"
branch_labels: str | Sequence[str] | None = None
depends_on: str | Sequence[str] | None = None


def upgrade() -> None:
    op.add_column(
        "alarms",
        sa.Column(
            "play_melody", sa.Boolean(), nullable=False, server_default=sa.true()
        ),
    )


def downgrade() -> None:
    op.drop_column("alarms", "play_melody")

use crate::render::{ctx::Ctx, feature::Feature};
use postgres::Row;

impl Ctx<'_> {
    pub fn legend_features(
        &self,
        layer_name: &str,
        mut cb: impl FnMut() -> Result<Vec<Row>, postgres::Error>,
    ) -> Result<Vec<Feature>, postgres::Error> {
        let _span = tracy_client::span!("legend_features");

        let Some(legend) = self.legend else {
            return Ok(cb()?.into_iter().map(|row| row.into()).collect());
        };

        let Some(legend) = legend.get(layer_name) else {
            return Ok(vec![]);
        };

        Ok(legend
            .iter()
            .map(|props| Feature::LegendData(props.clone()))
            .collect())
    }
}

//! Subsonic internet radio: everyone lists the stations (apps play the station URL directly); only admins
//! add, edit or delete them. Same stations as the BoogieBox UI.

use boogiebox_db::radio_stations::{self as rdb, StationInput};

use super::{
    handlers::{db_err, required_id, Ctx},
    response::{Node, SubsonicError},
};

fn station_node(s: &rdb::RadioStation) -> Node {
    Node::new("x")
        .attr("id", s.id.as_str())
        .attr("name", s.name.as_str())
        .attr("streamUrl", s.stream_url.as_str())
        .opt("homePageUrl", s.homepage_url.clone())
}

/// `getInternetRadioStations`.
pub async fn stations(ctx: &Ctx) -> Result<Node, SubsonicError> {
    let stations = ctx
        .db(|conn| rdb::list_stations(conn).map_err(db_err))
        .await?;
    Ok(
        Node::new("payload").object(Node::new("internetRadioStations").list(
            "internetRadioStation",
            stations.iter().map(station_node).collect(),
        )),
    )
}

fn admin_only(ctx: &Ctx) -> Result<(), SubsonicError> {
    if ctx.user.is_admin() {
        Ok(())
    } else {
        Err(SubsonicError::not_authorized())
    }
}

fn input(ctx: &Ctx) -> Result<StationInput, SubsonicError> {
    let p = &ctx.params;
    StationInput::new(
        p.required("name")?,
        p.required("streamUrl")?,
        p.text("homepageUrl").or(p.text("homePageUrl")),
    )
    .map_err(|e| SubsonicError::generic(e.message()))
}

/// `createInternetRadioStation` (admin).
pub async fn create(ctx: &Ctx) -> Result<Node, SubsonicError> {
    admin_only(ctx)?;
    let station = input(ctx)?;
    ctx.db(move |conn| {
        rdb::create_station(conn, &station)
            .map(|_| ())
            .map_err(db_err)
    })
    .await?;
    Ok(Node::new("payload"))
}

/// `updateInternetRadioStation` (admin).
pub async fn update(ctx: &Ctx) -> Result<Node, SubsonicError> {
    admin_only(ctx)?;
    let id = required_id(&ctx.params, "id")?;
    let station = input(ctx)?;
    ctx.db(move |conn| {
        rdb::update_station(conn, &id, &station)
            .map_err(db_err)?
            .map(|_| ())
            .ok_or_else(|| SubsonicError::not_found("Radio station"))
    })
    .await?;
    Ok(Node::new("payload"))
}

/// `deleteInternetRadioStation` (admin).
pub async fn delete(ctx: &Ctx) -> Result<Node, SubsonicError> {
    admin_only(ctx)?;
    let id = required_id(&ctx.params, "id")?;
    let removed = ctx
        .db(move |conn| rdb::delete_station(conn, &id).map_err(db_err))
        .await?;
    if removed {
        Ok(Node::new("payload"))
    } else {
        Err(SubsonicError::not_found("Radio station"))
    }
}
